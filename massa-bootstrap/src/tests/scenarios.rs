// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::{
    get_boot_state, get_peers, get_random_final_state_bootstrap, get_random_ledger_changes,
};
use crate::listener::PollEvent;
use crate::tests::tools::{
    get_random_async_pool_changes, get_random_executed_de_changes, get_random_executed_ops_changes,
    get_random_pos_changes, make_runtime,
};
use crate::{
    client::MockBSConnector, get_state, server::MockBSEventPoller, start_bootstrap_server,
    tests::tools::get_bootstrap_config,
};
use crate::{BootstrapConfig, BootstrapManager, BootstrapTcpListener};
use massa_async_pool::AsyncPoolConfig;
use massa_consensus_exports::{
    bootstrapable_graph::BootstrapableGraph, test_exports::MockConsensusControllerImpl,
};
use massa_db::{DBBatch, MassaDB, MassaDBConfig};
use massa_executed_ops::{ExecutedDenunciationsConfig, ExecutedOpsConfig};
use massa_final_state::{
    test_exports::assert_eq_final_state, FinalState, FinalStateConfig, StateChanges,
};
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_ledger_exports::LedgerConfig;
use massa_models::config::{
    DENUNCIATION_EXPIRE_PERIODS, ENDORSEMENT_COUNT, MAX_DEFERRED_CREDITS_LENGTH,
    MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH,
    MIP_STORE_STATS_BLOCK_CONSIDERED, MIP_STORE_STATS_COUNTERS_MAX,
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
use std::collections::{BTreeMap, HashMap};
use std::net::{SocketAddr, TcpStream};
use std::println;
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
    let db_config = MassaDBConfig {
        path: temp_dir.path().to_path_buf(),
        max_history_length: 10,
        thread_count,
    };
    let db = Arc::new(RwLock::new(MassaDB::new(db_config)));
    let final_state_local_config = FinalStateConfig {
        ledger_config: LedgerConfig {
            thread_count,
            initial_ledger_path: "".into(),
            disk_ledger_path: temp_dir.path().to_path_buf(),
            max_key_length: MAX_DATASTORE_KEY_LENGTH,
            max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        },
        async_pool_config: AsyncPoolConfig {
            thread_count,
            max_length: MAX_ASYNC_POOL_LENGTH,
            max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
            max_key_length: MAX_DATASTORE_KEY_LENGTH as u32,
        },
        pos_config: PoSConfig {
            periods_per_cycle,
            thread_count,
            cycle_history_length: POS_SAVED_CYCLES,
            max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
            max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
            max_credit_length: MAX_DEFERRED_CREDITS_LENGTH,
        },
        executed_ops_config: ExecutedOpsConfig { thread_count },
        final_history_length: 100,
        initial_seed_string: "".into(),
        initial_rolls_path: "".into(),
        thread_count,
        periods_per_cycle,
        executed_denunciations_config: ExecutedDenunciationsConfig {
            denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
            thread_count,
            endorsement_count: ENDORSEMENT_COUNT,
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
            db.clone(),
        )
        .unwrap(),
        final_state_local_config.clone(),
        db.clone(),
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

    start_bootstrap_server(
        BootstrapTcpListener::new(&addr).unwrap().1,
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
    let _bs_manager = mock_bootstrap_manager(addr.clone(), config.clone());

    let conn = TcpStream::connect(addr);
    conn.unwrap();
}

//#[ignore]
#[test]
fn test_bootstrap_server() {
    let thread_count = 2;
    let periods_per_cycle = 2;
    let (bootstrap_config, keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;
    let rolls_path = PathBuf::from_str("../massa-node/base_config/initial_rolls.json").unwrap();
    let genesis_address = Address::from_public_key(&KeyPair::generate().get_public_key());

    // let (consensus_controller, mut consensus_event_receiver) =
    //     MockConsensusController::new_with_receiver();
    // let (network_cmd_tx, mut network_cmd_rx) = mpsc::channel::<NetworkCommand>(5);

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
    let temp_dir_server = TempDir::new().unwrap();
    let db_server_config = MassaDBConfig {
        path: temp_dir_server.path().to_path_buf(),
        max_history_length: 100,
        thread_count,
    };
    let db_server = Arc::new(RwLock::new(MassaDB::new(db_server_config)));
    let temp_dir_client = TempDir::new().unwrap();
    let db_client_config = MassaDBConfig {
        path: temp_dir_client.path().to_path_buf(),
        max_history_length: 100,
        thread_count,
    };
    let db_client = Arc::new(RwLock::new(MassaDB::new(db_client_config)));
    let final_state_local_config = FinalStateConfig {
        ledger_config: LedgerConfig {
            thread_count,
            initial_ledger_path: "".into(),
            disk_ledger_path: temp_dir_server.path().to_path_buf(),
            max_key_length: MAX_DATASTORE_KEY_LENGTH,
            max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        },
        async_pool_config: AsyncPoolConfig {
            thread_count,
            max_length: MAX_ASYNC_POOL_LENGTH,
            max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
            max_key_length: MAX_DATASTORE_KEY_LENGTH as u32,
        },
        pos_config: PoSConfig {
            periods_per_cycle,
            thread_count,
            cycle_history_length: POS_SAVED_CYCLES,
            max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
            max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
            max_credit_length: MAX_DEFERRED_CREDITS_LENGTH,
        },
        executed_ops_config: ExecutedOpsConfig { thread_count },
        executed_denunciations_config: ExecutedDenunciationsConfig {
            denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
            thread_count,
            endorsement_count: ENDORSEMENT_COUNT,
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

    let pos_server = PoSFinalState::new(
        final_state_local_config.pos_config.clone(),
        "",
        &rolls_path,
        server_selector_controller.clone(),
        Hash::from_bytes(&[0; HASH_SIZE_BYTES]),
        db_server.clone(),
    );

    // setup final states
    let final_state_server = Arc::new(RwLock::new(get_random_final_state_bootstrap(
        pos_server.unwrap(),
        final_state_local_config.clone(),
        db_server.clone(),
    )));

    let mut cur_slot: Slot = Slot::new(0, thread_count - 1);
    for _i in 0..11 {
        final_state_server
            .write()
            .db
            .write()
            .write_changes(BTreeMap::new(), Some(cur_slot), false)
            .unwrap();
        cur_slot = cur_slot.get_next_slot(thread_count).unwrap();
    }

    let final_state_client = Arc::new(RwLock::new(FinalState::create_final_state(
        PoSFinalState::new(
            final_state_local_config.pos_config.clone(),
            "",
            &rolls_path,
            client_selector_controller.clone(),
            Hash::from_bytes(&[0; HASH_SIZE_BYTES]),
            db_client.clone(),
        )
        .unwrap(),
        final_state_local_config,
        db_client.clone(),
    )));

    // setup final state mocks.
    // TODO: work out a way to handle the clone shenanigans in a cleaner manner
    let final_state_client_clone = final_state_client.clone();
    let final_state_server_clone1 = final_state_server.clone();
    let final_state_server_clone2 = final_state_server.clone();

    let (mock_bs_listener, mock_remote_connector) = conn_establishment_mocks();
    // Setup network command mock-story: hard-code the result of getting bootstrap peers
    let mut mocked1 = MockProtocolController::new();
    let mut mocked2 = Box::new(MockProtocolController::new());
    mocked2
        .expect_get_bootstrap_peers()
        .times(1)
        .returning(|| Ok(get_peers(&keypair.clone())));

    mocked1.expect_clone_box().return_once(move || mocked2);

    let mut stream_mock1 = Box::new(MockConsensusControllerImpl::new());
    let mut stream_mock2 = Box::new(MockConsensusControllerImpl::new());
    let mut stream_mock3 = Box::new(MockConsensusControllerImpl::new());
    let mut seq = mockall::Sequence::new();

    let sent_graph = get_boot_state();
    let sent_graph_clone = sent_graph.clone();
    stream_mock3
        .expect_get_bootstrap_part()
        .times(1)
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

    stream_mock2
        .expect_clone_box()
        .return_once(move || stream_mock3);
    stream_mock1
        .expect_clone_box()
        .return_once(move || stream_mock2);

    let cloned_store = mip_store.clone();

    // Start the bootstrap server thread
    let bootstrap_manager_thread = std::thread::Builder::new()
        .name("bootstrap_thread".to_string())
        .spawn(move || {
            start_bootstrap_server(
                mock_bs_listener,
                stream_mock1,
                Box::new(mocked1),
                final_state_server_clone1,
                bootstrap_config.clone(),
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
            let mut current_slot = Slot::new(5, 1);

            for _ in 0..10 {
                std::thread::sleep(Duration::from_millis(500));
                let mut final_write = final_state_server_clone2.write();
                let next = current_slot.get_next_slot(thread_count).unwrap();

                final_write.slot = next;

                println!("ADDING CHANGES for slot {:?}", next);
                let changes = StateChanges {
                    pos_changes: get_random_pos_changes(10),
                    ledger_changes: get_random_ledger_changes(10),
                    async_pool_changes: get_random_async_pool_changes(10, thread_count),
                    executed_ops_changes: get_random_executed_ops_changes(10),
                    executed_denunciations_changes: get_random_executed_de_changes(10),
                };

                let mut batch = DBBatch::new();

                // TODO: UNCOMMENT AND DEAL WITH ERROR
                /*final_write
                .pos_state
                .apply_changes_to_batch(changes.pos_changes.clone(), next, false, &mut batch)
                .unwrap();*/
                final_write
                    .ledger
                    .apply_changes_to_batch(changes.ledger_changes.clone(), &mut batch);
                final_write
                    .async_pool
                    .apply_changes_to_batch(&changes.async_pool_changes, &mut batch);
                final_write.executed_ops.apply_changes_to_batch(
                    changes.executed_ops_changes.clone(),
                    next,
                    &mut batch,
                );
                final_write.executed_denunciations.apply_changes_to_batch(
                    changes.executed_denunciations_changes.clone(),
                    next,
                    &mut batch,
                );

                final_write.db.write().write_batch(batch, Some(next));

                final_write.db.write().cur_change_id = next;

                let mut list_changes_write = list_changes_clone.write();
                list_changes_write.push((next, changes));

                current_slot = next;
            }
        })
        .unwrap();

    // launch the get_state process
    let bootstrap_res = make_runtime()
        .block_on(get_state(
            bootstrap_config,
            final_state_client_clone,
            mock_remote_connector,
            Version::from_str("TEST.1.10").unwrap(),
            MassaTime::now().unwrap().saturating_sub(1000.into()),
            None,
            None,
        ))
        .unwrap();

    // Make sure the modifier thread has done its job
    mod_thread.join().unwrap();

    {
        let mut final_state_client_write = final_state_client.write();
        final_state_client_write.recompute_caches();
        final_state_client_write.init_ledger_hash();
    }

    let slot_client = final_state_client.read().db.read().get_change_id();
    let slot_server = final_state_server.read().db.read().get_change_id();

    println!("slot client: {:?}", slot_client);
    println!("slot server: {:?}", slot_server);

    let hash_client = final_state_client
        .read()
        .db
        .read()
        .compute_hash_from_scratch();
    let hash_server = final_state_server
        .read()
        .db
        .read()
        .compute_hash_from_scratch();

    println!("hash client: {:?}", hash_client);
    println!("hash server: {:?}", hash_server);

    // check final states
    assert_eq_final_state(&final_state_server.read(), &final_state_client.read());

    // TODO: UNCOMMENT AND DEAL WITH ERROR
    //assert_eq_final_state_hash(&final_state_server.read(), &final_state_client.read());

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
    // TODO: UNCOMMENT AND DEAL WITH ERROR
    //assert_eq_bootstrap_graph(&sent_graph, &bootstrap_res.graph.unwrap());

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

fn conn_establishment_mocks() -> (MockBSEventPoller, MockBSConnector) {
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
    // TODO: Why is it twice, and not just once?
    let mut seq = Sequence::new();
    let mut mock_bs_listener = MockBSEventPoller::new();
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

    let mut seq = Sequence::new();
    let mut mock_remote_connector = MockBSConnector::new();
    mock_remote_connector
        .expect_connect_timeout()
        .times(1)
        .returning(move |_, _| Ok(std::net::TcpStream::connect("127.0.0.1:8069").unwrap()))
        .in_sequence(&mut seq);
    (mock_bs_listener, mock_remote_connector)
}
