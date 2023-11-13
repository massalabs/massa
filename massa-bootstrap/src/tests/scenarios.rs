// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::{get_random_final_state_bootstrap, BASE_BOOTSTRAP_IP};
use super::universe_client::{BootstrapClientForeignControllers, BootstrapClientTestUniverse};
use super::universe_server::{
    BootstrapServerForeignControllers, BootstrapServerTestUniverse,
    BootstrapServerTestUniverseBuilder,
};
use crate::listener::PollEvent;
use crate::BootstrapError;
use crate::{client::MockBSConnector, start_bootstrap_server, tests::tools::get_bootstrap_config};
use crate::{listener::MockBootstrapTcpListener, BootstrapConfig, BootstrapTcpListener};
use massa_async_pool::AsyncPoolConfig;
use massa_consensus_exports::{bootstrapable_graph::BootstrapableGraph, MockConsensusController};
use massa_db_exports::{DBBatch, MassaDBConfig, MassaDBController};
use massa_db_worker::MassaDB;
use massa_executed_ops::{ExecutedDenunciationsConfig, ExecutedOpsConfig};
use massa_final_state::FinalStateConfig;
use massa_ledger_exports::{LedgerChanges, LedgerConfig, LedgerController};
use massa_ledger_worker::FinalLedger;
use massa_metrics::MassaMetrics;
use massa_models::amount::Amount;
use massa_models::config::{
    DENUNCIATION_EXPIRE_PERIODS, ENDORSEMENT_COUNT, GENESIS_TIMESTAMP,
    KEEP_EXECUTED_HISTORY_EXTRA_PERIODS, MAX_BOOTSTRAP_FINAL_STATE_PARTS_SIZE,
    MAX_BOOTSTRAP_VERSIONING_ELEMENTS_SIZE, MAX_DEFERRED_CREDITS_LENGTH,
    MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH, T0,
    THREAD_COUNT,
};
use massa_models::{
    address::Address, config::MAX_DATASTORE_VALUE_LENGTH, node::NodeId,
    streaming_step::StreamingStep, version::Version,
};
use massa_models::{
    config::{
        MAX_ASYNC_POOL_LENGTH, MAX_DATASTORE_KEY_LENGTH, MAX_FUNCTION_NAME_LENGTH,
        MAX_PARAMETERS_SIZE, POS_SAVED_CYCLES,
    },
    prehash::PreHashSet,
};

use massa_pos_exports::{PoSConfig, PoSFinalState, SelectorConfig};
use massa_pos_worker::start_selector_worker;
use massa_protocol_exports::{BootstrapPeers, MockProtocolController};
use massa_signature::KeyPair;
use massa_test_framework::TestUniverse;
use massa_versioning::versioning::{MipStatsConfig, MipStore};
use mockall::Sequence;
use num::rational::Ratio;
use parking_lot::RwLock;
use serial_test::serial;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::mpsc::Receiver;
use std::vec;
use std::{path::PathBuf, str::FromStr, sync::Arc, time::Duration};
use tempfile::{NamedTempFile, TempDir};

lazy_static::lazy_static! {
    pub static ref BOOTSTRAP_CONFIG_KEYPAIR: (BootstrapConfig, KeyPair) = {
        let keypair = KeyPair::generate(0).unwrap();
        (get_bootstrap_config(NodeId::new(keypair.get_public_key())), keypair)
    };
}

#[test]
#[serial]
fn test_bootstrap_not_whitelisted() {
    let port = 8069;
    let server_keypair = KeyPair::generate(0).unwrap();
    let mut bootstrap_server_config = BootstrapConfig::default();
    bootstrap_server_config.bootstrap_whitelist_path =
        PathBuf::from("../massa-node/base_config/bootstrap_whitelist.json");
    let server_universe = BootstrapServerTestUniverseBuilder::new()
        .set_port(port)
        .set_config(bootstrap_server_config)
        .set_keypair(&server_keypair)
        .build();
    let mut client_universe = BootstrapClientTestUniverse::new(
        BootstrapClientForeignControllers::new_with_mocks(),
        BootstrapConfig::default(),
    );
    match client_universe.launch_bootstrap(port, NodeId::new(server_keypair.get_public_key())) {
        Ok(()) => panic!("Bootstrap should have failed"),
        Err(BootstrapError::ReceivedError(err)) => {
            assert_eq!(err, String::from("IP 127.0.0.1 is not in the whitelist"))
        }
        Err(err) => panic!("Unexpected error: {:?}", err),
    }
    drop(server_universe);
}

#[test]
#[serial]
fn test_bootstrap_server() {
    let port = 8070;
    let server_keypair = KeyPair::generate(0).unwrap();
    let address = Address::from_public_key(&server_keypair.get_public_key());
    let server_universe = BootstrapServerTestUniverseBuilder::new()
        .set_port(port)
        .set_keypair(&server_keypair)
        .set_address_balance(&address, Amount::from_mantissa_scale(100, 0).unwrap())
        // FUTURE: set_ledger_changes or add a slot param to methods like `set_address_balance`
        .build();
    let mut client_universe = BootstrapClientTestUniverse::new(
        BootstrapClientForeignControllers::new_with_mocks(),
        BootstrapConfig::default(),
    );
    match client_universe.launch_bootstrap(port, NodeId::new(server_keypair.get_public_key())) {
        Ok(()) => (),
        Err(err) => panic!("Unexpected error: {:?}", err),
    }
    client_universe.compare_ledger(server_universe.database.clone());
}

// Regression test for Issue #3932
#[test]
#[serial]
fn test_bootstrap_accept_err() {
    let thread_count = 2;
    let periods_per_cycle = 2;
    let (bootstrap_config, keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;
    let rolls_path = PathBuf::from_str("../massa-node/base_config/initial_rolls.json").unwrap();
    let genesis_address = Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());

    // setup final state local config
    let temp_dir_server = TempDir::new().unwrap();
    let db_server_config = MassaDBConfig {
        path: temp_dir_server.path().to_path_buf(),
        max_history_length: 10,
        max_final_state_elements_size: 100_000_000,
        max_versioning_elements_size: 100_000_000,
        thread_count,
    };
    let db_server = Arc::new(RwLock::new(
        Box::new(MassaDB::new(db_server_config)) as Box<(dyn MassaDBController + 'static)>
    ));
    let final_state_local_config = FinalStateConfig {
        ledger_config: LedgerConfig {
            thread_count,
            initial_ledger_path: "".into(),
            max_key_length: MAX_DATASTORE_KEY_LENGTH,
            max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        },
        async_pool_config: AsyncPoolConfig {
            thread_count,
            max_length: MAX_ASYNC_POOL_LENGTH,
            max_function_length: MAX_FUNCTION_NAME_LENGTH,
            max_function_params_length: MAX_PARAMETERS_SIZE as u64,
            max_key_length: MAX_DATASTORE_KEY_LENGTH as u32,
        },
        pos_config: PoSConfig {
            periods_per_cycle,
            thread_count,
            cycle_history_length: POS_SAVED_CYCLES,
            max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
            max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
            max_credit_length: MAX_DEFERRED_CREDITS_LENGTH,
            initial_deferred_credits_path: None,
        },
        executed_ops_config: ExecutedOpsConfig {
            thread_count,
            keep_executed_history_extra_periods: KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
        },
        executed_denunciations_config: ExecutedDenunciationsConfig {
            denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
            thread_count,
            endorsement_count: ENDORSEMENT_COUNT,
            keep_executed_history_extra_periods: KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
        },
        final_history_length: 100,
        initial_seed_string: "".into(),
        initial_rolls_path: "".into(),
        endorsement_count: ENDORSEMENT_COUNT,
        max_executed_denunciations_length: 1000,
        thread_count,
        periods_per_cycle,
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        t0: T0,
        genesis_timestamp: *GENESIS_TIMESTAMP,
    };

    // setup selector local config
    let selector_local_config = SelectorConfig {
        thread_count,
        periods_per_cycle,
        genesis_address,
        ..Default::default()
    };

    // start proof-of-stake selectors
    let (_, server_selector_controller) = start_selector_worker(selector_local_config)
        .expect("could not start server selector controller");

    let pos_server = PoSFinalState::new(
        final_state_local_config.pos_config.clone(),
        "",
        &rolls_path,
        server_selector_controller.clone(),
        db_server.clone(),
    );

    // setup final states
    let final_state_server = Arc::new(RwLock::new(get_random_final_state_bootstrap(
        pos_server.unwrap(),
        final_state_local_config,
        db_server.clone(),
    )));

    // mock story: 1. accept() -> error. 2. accept() -> stop
    let (mock_bs_listener, _mock_remote_connector) = accept_err_accept_stop_mocks();

    let mut mocked_proto_ctrl = MockProtocolController::new();
    mocked_proto_ctrl
        .expect_clone_box()
        .return_once(move || Box::new(MockProtocolController::new()));

    let stream_mock1 = Box::new(MockConsensusController::new());

    // Start the bootstrap server thread. The expectation for an err then stop is the test.
    // By ensuring that there is a call to poll following an accept err, it shows that the server
    // will still listen following an accept err.
    let bootstrap_manager_thread = std::thread::Builder::new()
        .name("bootstrap_thread".to_string())
        .spawn(move || {
            let (listener_stopper, _) =
                BootstrapTcpListener::create(&"127.0.0.1:0".parse().unwrap()).unwrap();
            start_bootstrap_server(
                mock_bs_listener,
                listener_stopper,
                stream_mock1,
                Box::new(mocked_proto_ctrl),
                final_state_server,
                bootstrap_config.clone(),
                keypair.clone(),
                Version::from_str("TEST.1.10").unwrap(),
                MassaMetrics::new(
                    false,
                    "0.0.0.0:31248".parse().unwrap(),
                    thread_count,
                    Duration::from_secs(5),
                )
                .0,
            )
            .unwrap()
        })
        .unwrap();

    // stop bootstrap server
    bootstrap_manager_thread
        .join()
        .unwrap()
        .stop()
        .expect("could not stop bootstrap server");
}

fn accept_err_accept_stop_mocks() -> (MockBootstrapTcpListener, MockBSConnector) {
    // first an error...
    let mut seq = Sequence::new();
    let mut mock_bs_listener = MockBootstrapTcpListener::new();
    mock_bs_listener
        .expect_poll()
        .times(1)
        // Mock the `accept` method here by receiving from the listen-loop thread
        .returning(move || {
            Err(BootstrapError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "mocked error",
            )))
        })
        .in_sequence(&mut seq);
    // ... then a stop
    mock_bs_listener
        .expect_poll()
        .times(1)
        // Mock the `accept` method here by receiving from the listen-loop thread
        .returning(move || Ok(PollEvent::Stop))
        .in_sequence(&mut seq);

    (mock_bs_listener, MockBSConnector::new())
}
