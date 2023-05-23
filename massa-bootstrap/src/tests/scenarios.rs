// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::{
    get_boot_state, get_peers, get_random_final_state_bootstrap, get_random_ledger_changes,
};
use crate::bindings::BootstrapClientBinder;
use crate::listener::PollEvent;
use crate::tests::tools::{
    get_random_async_pool_changes, get_random_executed_de_changes, get_random_executed_ops_changes,
    get_random_pos_changes,
};
use crate::{
    client::MockBSConnector,
    get_state,
    listener::MockBootstrapTcpListener,
    start_bootstrap_server,
    tests::tools::{assert_eq_bootstrap_graph, get_bootstrap_config},
    BootstrapConfig, BootstrapManager, BootstrapTcpListener,
};
use massa_async_pool::AsyncPoolConfig;
use massa_consensus_exports::{
    bootstrapable_graph::BootstrapableGraph, test_exports::MockConsensusControllerImpl,
};
use massa_executed_ops::{ExecutedDenunciationsConfig, ExecutedOpsConfig};
use massa_final_state::{
    test_exports::{assert_eq_final_state, assert_eq_final_state_hash},
    FinalState, FinalStateConfig, StateChanges,
};
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_ledger_exports::LedgerConfig;
use massa_models::config::{
    DENUNCIATION_EXPIRE_PERIODS, ENDORSEMENT_COUNT, EXECUTED_OPS_BOOTSTRAP_PART_SIZE,
    MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MIP_STORE_STATS_BLOCK_CONSIDERED,
    MIP_STORE_STATS_COUNTERS_MAX,
};
use massa_models::{
    address::Address, config::MAX_DATASTORE_VALUE_LENGTH, node::NodeId, slot::Slot,
    streaming_step::StreamingStep, version::Version,
};
use massa_models::{
    config::{
        MAX_ASYNC_MESSAGE_DATA, MAX_ASYNC_POOL_LENGTH, MAX_DATASTORE_KEY_LENGTH, POS_SAVED_CYCLES,
    },
    prehash::PreHashSet,
};

use massa_pos_exports::{
    test_exports::assert_eq_pos_selection, PoSConfig, PoSFinalState, SelectorConfig,
};
use massa_pos_worker::start_selector_worker;
use massa_protocol_exports::MockProtocolController;
use massa_signature::KeyPair;
use massa_time::MassaTime;
use massa_versioning_worker::versioning::{
    MipComponent, MipInfo, MipState, MipStatsConfig, MipStore,
};
use mockall::Sequence;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::{Condvar, Mutex};
use std::{path::PathBuf, str::FromStr, sync::Arc, time::Duration};
use tempfile::TempDir;

lazy_static::lazy_static! {
    pub static ref BOOTSTRAP_CONFIG_KEYPAIR: (BootstrapConfig, KeyPair) = {
        let keypair = KeyPair::generate();
        (get_bootstrap_config(NodeId::new(keypair.get_public_key())), keypair)
    };
}

fn mock_bootstrap_manager(addr: SocketAddr, bootstrap_config: BootstrapConfig) -> BootstrapManager {
    // TODO from config
    let rolls_path = PathBuf::from_str("../massa-node/base_config/initial_rolls.json").unwrap();
    let thread_count = 2;
    let periods_per_cycle = 2;
    let genesis_address = Address::from_public_key(&KeyPair::generate().get_public_key());
    // setup selector local config
    let selector_local_config = SelectorConfig {
        thread_count,
        periods_per_cycle,
        genesis_address,
        ..Default::default()
    };

    // create a MIP store
    let mip_stats_cfg = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        counters_max: MIP_STORE_STATS_COUNTERS_MAX,
    };
    let mi_1 = MipInfo {
        name: "MIP-0002".to_string(),
        version: 2,
        components: HashMap::from([(MipComponent::Address, 1)]),
        start: MassaTime::from(5),
        timeout: MassaTime::from(10),
        activation_delay: MassaTime::from(4),
    };
    let state_1 = MipState::new(MassaTime::from(3));
    let mip_store = MipStore::try_from(([(mi_1, state_1)], mip_stats_cfg.clone())).unwrap();

    // start bootstrap manager
    let (_, keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;
    let mut mocked1 = Box::new(MockProtocolController::new());
    let mocked2 = Box::new(MockProtocolController::new());
    mocked1.expect_clone_box().return_once(move || mocked2);

    // start proof-of-stake selectors
    let (_server_selector_manager, server_selector_controller) =
        start_selector_worker(selector_local_config.clone())
            .expect("could not start server selector controller");

    // setup final state local config
    let temp_dir = TempDir::new().unwrap();
    let final_state_local_config = FinalStateConfig {
        ledger_config: LedgerConfig {
            thread_count,
            initial_ledger_path: "".into(),
            disk_ledger_path: temp_dir.path().to_path_buf(),
            max_key_length: MAX_DATASTORE_KEY_LENGTH,
            max_ledger_part_size: 100_000,
            max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        },
        async_pool_config: AsyncPoolConfig {
            thread_count,
            max_length: MAX_ASYNC_POOL_LENGTH,
            max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
            bootstrap_part_size: 100,
        },
        pos_config: PoSConfig {
            periods_per_cycle,
            thread_count,
            cycle_history_length: POS_SAVED_CYCLES,
            credits_bootstrap_part_size: 100,
        },
        executed_ops_config: ExecutedOpsConfig {
            thread_count,
            bootstrap_part_size: 10,
        },
        final_history_length: 100,
        initial_seed_string: "".into(),
        initial_rolls_path: "".into(),
        thread_count,
        periods_per_cycle,
        executed_denunciations_config: ExecutedDenunciationsConfig {
            denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
            bootstrap_part_size: EXECUTED_OPS_BOOTSTRAP_PART_SIZE,
        },
        endorsement_count: ENDORSEMENT_COUNT,
        max_executed_denunciations_length: 1000,
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
    };

    let final_state_server = Arc::new(RwLock::new(get_random_final_state_bootstrap(
        PoSFinalState::new(
            final_state_local_config.pos_config.clone(),
            "",
            &rolls_path,
            server_selector_controller.clone(),
            Hash::from_bytes(&[0; HASH_SIZE_BYTES]),
        )
        .unwrap(),
        final_state_local_config.clone(),
    )));
    let mut stream_mock1 = Box::new(MockConsensusControllerImpl::new());
    let mut stream_mock2 = Box::new(MockConsensusControllerImpl::new());
    let stream_mock3 = Box::new(MockConsensusControllerImpl::new());
    stream_mock2
        .expect_clone_box()
        .return_once(move || stream_mock3);
    stream_mock1
        .expect_clone_box()
        .return_once(move || stream_mock2);

    let (waker, mut _listener) = BootstrapTcpListener::create(&addr).unwrap();
    let mut listener = MockBootstrapTcpListener::new();
    listener
        .expect_poll()
        .times(1)
        .returning(move || _listener.poll());
    listener.expect_poll().return_once(|| Ok(PollEvent::Stop));
    start_bootstrap_server(
        listener,
        waker,
        stream_mock1,
        mocked1,
        final_state_server,
        bootstrap_config.clone(),
        keypair.clone(),
        Version::from_str("TEST.1.10").unwrap(),
        mip_store,
    )
    .unwrap()
}

#[test]
fn test_bootstrap_whitelist() {
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let (config, _keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;
    let mut config = config.clone();
    config.bootstrap_whitelist_path = Path::new("./test_fixtures/local.json").to_path_buf();
    let bs_manager = mock_bootstrap_manager(addr.clone(), config.clone());

    let mut binding = BootstrapClientBinder::new(
        TcpStream::connect(&addr).unwrap(),
        _keypair.get_public_key(),
        (&config.clone()).into(),
    );
    binding
        .handshake(Version::from_str("TEST.1.10").unwrap())
        .unwrap();

    let _ = bs_manager.stop().unwrap();
    std::thread::sleep(Duration::from_millis(20));
}

#[test]
fn test_bootstrap_server() {
    let thread_count = 2;
    let periods_per_cycle = 2;
    let (bootstrap_config, keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;
    let rolls_path = PathBuf::from_str("../massa-node/base_config/initial_rolls.json").unwrap();
    let genesis_address = Address::from_public_key(&KeyPair::generate().get_public_key());

    // create a MIP store
    let mip_stats_cfg = MipStatsConfig {
        block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
        counters_max: MIP_STORE_STATS_COUNTERS_MAX,
    };
    let mi_1 = MipInfo {
        name: "MIP-0002".to_string(),
        version: 2,
        components: HashMap::from([(MipComponent::Address, 1)]),
        start: MassaTime::from(5),
        timeout: MassaTime::from(10),
        activation_delay: MassaTime::from(4),
    };
    let state_1 = MipState::new(MassaTime::from(3));
    let mip_store = MipStore::try_from(([(mi_1, state_1)], mip_stats_cfg.clone())).unwrap();

    // setup final state local config
    let temp_dir = TempDir::new().unwrap();
    let final_state_local_config = FinalStateConfig {
        ledger_config: LedgerConfig {
            thread_count,
            initial_ledger_path: "".into(),
            disk_ledger_path: temp_dir.path().to_path_buf(),
            max_key_length: MAX_DATASTORE_KEY_LENGTH,
            max_ledger_part_size: 100_000,
            max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        },
        async_pool_config: AsyncPoolConfig {
            thread_count,
            max_length: MAX_ASYNC_POOL_LENGTH,
            max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
            bootstrap_part_size: 100,
        },
        pos_config: PoSConfig {
            periods_per_cycle,
            thread_count,
            cycle_history_length: POS_SAVED_CYCLES,
            credits_bootstrap_part_size: 100,
        },
        executed_ops_config: ExecutedOpsConfig {
            thread_count,
            bootstrap_part_size: 10,
        },
        executed_denunciations_config: ExecutedDenunciationsConfig {
            denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
            bootstrap_part_size: 10,
        },
        final_history_length: 100,
        initial_seed_string: "".into(),
        initial_rolls_path: "".into(),
        endorsement_count: ENDORSEMENT_COUNT,
        max_executed_denunciations_length: 1000,
        thread_count,
        periods_per_cycle,
        max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
    };

    // setup selector local config
    let selector_local_config = SelectorConfig {
        thread_count,
        periods_per_cycle,
        genesis_address,
        ..Default::default()
    };

    // start proof-of-stake selectors
    let (mut server_selector_manager, server_selector_controller) =
        start_selector_worker(selector_local_config.clone())
            .expect("could not start server selector controller");
    let (mut client_selector_manager, client_selector_controller) =
        start_selector_worker(selector_local_config)
            .expect("could not start client selector controller");

    // setup final states
    let final_state_server = Arc::new(RwLock::new(get_random_final_state_bootstrap(
        PoSFinalState::new(
            final_state_local_config.pos_config.clone(),
            "",
            &rolls_path,
            server_selector_controller.clone(),
            Hash::from_bytes(&[0; HASH_SIZE_BYTES]),
        )
        .unwrap(),
        final_state_local_config.clone(),
    )));
    let final_state_client = Arc::new(RwLock::new(FinalState::create_final_state(
        PoSFinalState::new(
            final_state_local_config.pos_config.clone(),
            "",
            &rolls_path,
            client_selector_controller.clone(),
            Hash::from_bytes(&[0; HASH_SIZE_BYTES]),
        )
        .unwrap(),
        final_state_local_config,
    )));

    // setup final state mocks.
    let final_state_client_clone = final_state_client.clone();
    let final_state_server_clone1 = final_state_server.clone();
    let final_state_server_clone2 = final_state_server.clone();

    let (mock_bs_listener, mock_remote_connector) = conn_establishment_mocks();
    // Setup network command mock-story: hard-code the result of getting bootstrap peers
    let mock_proto_ctrl = {
        let mut res = Box::new(MockProtocolController::new());
        res.expect_clone_box().return_once(move || {
            let mut story = Box::new(MockProtocolController::new());
            story
                .expect_get_bootstrap_peers()
                .times(1)
                .returning(|| Ok(get_peers(&keypair.clone())));
            story
        });
        res
    };

    let sent_graph = get_boot_state();
    let mock_stream = {
        let mut mock_story = Box::new(MockConsensusControllerImpl::new());
        let sent_graph_clone = sent_graph.clone();
        let mut seq = mockall::Sequence::new();
        mock_story
            .expect_get_bootstrap_part()
            .times(10)
            .in_sequence(&mut seq)
            .returning(move |_, slot| {
                if StreamingStep::Ongoing(Slot::new(1, 1)) == slot {
                    Ok((
                        sent_graph_clone.clone(),
                        PreHashSet::default(),
                        StreamingStep::Started,
                    ))
                } else {
                    Ok((
                        BootstrapableGraph {
                            final_blocks: vec![],
                        },
                        PreHashSet::default(),
                        StreamingStep::Finished(None),
                    ))
                }
            });

        let mut res = Box::new(MockConsensusControllerImpl::new());

        res.expect_clone_box().return_once(move || {
            let mut stream_mock2 = Box::new(MockConsensusControllerImpl::new());
            stream_mock2
                .expect_clone_box()
                .return_once(move || mock_story);
            stream_mock2
        });
        res
    };

    let cloned_store = mip_store.clone();

    let mut bootstrap_config_clone = bootstrap_config.clone();
    bootstrap_config_clone.bootstrap_whitelist_path =
        Path::new("./test_fixtures/local.json").to_path_buf();

    // Start the bootstrap server thread
    let bootstrap_manager_thread = std::thread::Builder::new()
        .name("bootstrap_thread".to_string())
        .spawn(move || {
            let (waker, _) = BootstrapTcpListener::create(&"127.0.0.1:0".parse().unwrap()).unwrap();
            start_bootstrap_server(
                mock_bs_listener,
                waker,
                mock_stream,
                mock_proto_ctrl,
                final_state_server_clone1,
                bootstrap_config_clone.clone(),
                keypair.clone(),
                Version::from_str("TEST.1.10").unwrap(),
                cloned_store,
            )
            .unwrap()
        })
        .unwrap();

    // launch the modifier thread
    let list_changes: Arc<RwLock<Vec<(Slot, StateChanges)>>> = Arc::new(RwLock::new(Vec::new()));
    let list_changes_clone = list_changes.clone();
    let mod_thread = std::thread::Builder::new()
        .name("modifier thread".to_string())
        .spawn(move || {
            for _ in 0..10 {
                std::thread::sleep(Duration::from_millis(500));
                let mut final_write = final_state_server_clone2.write();
                let next = final_write.slot.get_next_slot(thread_count).unwrap();
                final_write.slot = next;
                let changes = StateChanges {
                    pos_changes: get_random_pos_changes(10),
                    ledger_changes: get_random_ledger_changes(10),
                    async_pool_changes: get_random_async_pool_changes(10),
                    executed_ops_changes: get_random_executed_ops_changes(10),
                    executed_denunciations_changes: get_random_executed_de_changes(10),
                };
                final_write
                    .changes_history
                    .push_back((next, changes.clone()));
                let mut list_changes_write = list_changes_clone.write();
                list_changes_write.push((next, changes));
            }
        })
        .unwrap();

    // launch the get_state process
    let bootstrap_res = get_state(
        &bootstrap_config,
        final_state_client_clone,
        mock_remote_connector,
        Version::from_str("TEST.1.10").unwrap(),
        MassaTime::now().unwrap().saturating_sub(1000.into()),
        None,
        None,
        Arc::new((Mutex::new(false), Condvar::new())),
    )
    .unwrap();

    // apply the changes to the server state before matching with the client
    {
        let mut final_state_server_write = final_state_server.write();
        let list_changes_read = list_changes.read().clone();
        // note: skip the first change to match the update loop behaviour
        for (slot, change) in list_changes_read.iter().skip(1) {
            final_state_server_write
                .pos_state
                .apply_changes(change.pos_changes.clone(), *slot, false)
                .unwrap();
            final_state_server_write.ledger.apply_changes(
                change.ledger_changes.clone(),
                *slot,
                None,
            );
            final_state_server_write
                .async_pool
                .apply_changes_unchecked(&change.async_pool_changes);
            final_state_server_write
                .executed_ops
                .apply_changes(change.executed_ops_changes.clone(), *slot);
        }
    }
    // Make sure the modifier thread has done its job
    mod_thread.join().unwrap();

    // check final states
    assert_eq_final_state(&final_state_server.read(), &final_state_client.read());
    assert_eq_final_state_hash(&final_state_server.read(), &final_state_client.read());

    // compute initial draws
    final_state_server.write().compute_initial_draws().unwrap();
    final_state_client.write().compute_initial_draws().unwrap();

    // check selection draw
    let server_selection = server_selector_controller.get_entire_selection();
    let client_selection = client_selector_controller.get_entire_selection();
    assert_eq_pos_selection(&server_selection, &client_selection);

    // check peers
    assert_eq!(
        get_peers(&keypair).0,
        bootstrap_res.peers.unwrap().0,
        "mismatch between sent and received peers"
    );

    // check graphs
    assert_eq_bootstrap_graph(&sent_graph, &bootstrap_res.graph.unwrap());

    // check mip store
    let mip_raw_orig = mip_store.0.read().to_owned();
    let mip_raw_received = bootstrap_res.mip_store.unwrap().0.read().to_owned();
    assert_eq!(mip_raw_orig, mip_raw_received);

    // stop bootstrap server
    bootstrap_manager_thread
        .join()
        .unwrap()
        .stop()
        .expect("could not stop bootstrap server");

    // stop selector controllers
    server_selector_manager.stop();
    client_selector_manager.stop();
}

fn conn_establishment_mocks() -> (MockBootstrapTcpListener, MockBSConnector) {
    // Setup the server/client connection
    // Bind a TcpListener to localhost on a specific port
    let listener = std::net::TcpListener::bind("127.0.0.1:8069").unwrap();

    // Due to the limitations of the mocking system, the listener must loop-accept in a dedicated
    // thread. We use a channel to make the connection available in the mocked `accept` method
    let (conn_tx, conn_rx) = std::sync::mpsc::sync_channel(100);
    std::thread::Builder::new()
        .name("mock-listen-loop".to_string())
        .spawn(move || loop {
            conn_tx.send(listener.accept().unwrap()).unwrap()
        })
        .unwrap();

    // Mock the connection setups
    let mut seq = Sequence::new();
    let mut mock_bs_listener = MockBootstrapTcpListener::new();
    let mut mock_remote_connector = MockBSConnector::new();
    mock_remote_connector
        .expect_connect_timeout()
        .times(1)
        .returning(move |_, _| Ok(std::net::TcpStream::connect("127.0.0.1:8069").unwrap()))
        .in_sequence(&mut seq);
    mock_bs_listener
        .expect_poll()
        .times(1)
        // Mock the `accept` method here by receiving from the listen-loop thread
        .returning(move || Ok(PollEvent::NewConnection(conn_rx.recv().unwrap())))
        .in_sequence(&mut seq);
    mock_bs_listener
        .expect_poll()
        .times(1)
        // Mock the `accept` method here by receiving from the listen-loop thread
        .returning(move || Ok(PollEvent::Stop))
        .in_sequence(&mut seq);

    (mock_bs_listener, mock_remote_connector)
}
