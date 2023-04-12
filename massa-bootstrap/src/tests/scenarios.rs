// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::{
    mock_establisher,
    tools::{
        bridge_mock_streams, get_boot_state, get_peers, get_random_final_state_bootstrap,
        get_random_ledger_changes, wait_network_command,
    },
};
use crate::tests::tools::{
    get_random_async_pool_changes, get_random_executed_ops_changes, get_random_pos_changes,
};
use crate::BootstrapConfig;
use crate::{
    get_state, start_bootstrap_server,
    tests::tools::{assert_eq_bootstrap_graph, get_bootstrap_config},
};
use massa_async_pool::AsyncPoolConfig;
use massa_consensus_exports::{
    bootstrapable_graph::BootstrapableGraph,
    test_exports::{MockConsensusController, MockConsensusControllerMessage},
};
use massa_executed_ops::ExecutedOpsConfig;
use massa_final_state::{
    test_exports::{assert_eq_final_state, assert_eq_final_state_hash},
    FinalState, FinalStateConfig, StateChanges,
};
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_ledger_exports::LedgerConfig;
use massa_models::config::{MIP_STORE_STATS_BLOCK_CONSIDERED, MIP_STORE_STATS_COUNTERS_MAX};
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
use massa_network_exports::{NetworkCommand, NetworkCommandSender};
use massa_pos_exports::{
    test_exports::assert_eq_pos_selection, PoSConfig, PoSFinalState, SelectorConfig,
};
use massa_pos_worker::start_selector_worker;
use massa_signature::KeyPair;
use massa_time::MassaTime;
use massa_versioning_worker::versioning::{
    MipComponent, MipInfo, MipState, MipStatsConfig, MipStore,
};
use parking_lot::RwLock;
use serial_test::serial;
use std::{
    collections::HashMap,
    path::PathBuf,
    str::FromStr,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tempfile::TempDir;
use tokio::{net::TcpStream, sync::mpsc};

lazy_static::lazy_static! {
    pub static ref BOOTSTRAP_CONFIG_KEYPAIR: (BootstrapConfig, KeyPair) = {
        let keypair = KeyPair::generate();
        (get_bootstrap_config(NodeId::new(keypair.get_public_key())), keypair)
    };
}

#[tokio::test]
#[serial]
async fn test_bootstrap_server() {
    let thread_count = 2;
    let periods_per_cycle = 2;
    let (bootstrap_config, keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;
    let rolls_path = PathBuf::from_str("../massa-node/base_config/initial_rolls.json").unwrap();
    let genesis_address = Address::from_public_key(&KeyPair::generate().get_public_key());

    let (consensus_controller, mut consensus_event_receiver) =
        MockConsensusController::new_with_receiver();
    let (network_cmd_tx, mut network_cmd_rx) = mpsc::channel::<NetworkCommand>(5);

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
            max_key_length: MAX_DATASTORE_KEY_LENGTH as u32,
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
    let final_state_client_clone = final_state_client.clone();
    let final_state_server_clone = final_state_server.clone();

    // start bootstrap server
    let (mut mock_bs_listener, bootstrap_interface) = mock_establisher::new();
    let bootstrap_manager = start_bootstrap_server::<TcpStream>(
        consensus_controller,
        NetworkCommandSender(network_cmd_tx),
        final_state_server.clone(),
        bootstrap_config.clone(),
        mock_bs_listener
            .get_listener(&bootstrap_config.listen_addr.unwrap())
            .unwrap(),
        keypair.clone(),
        Version::from_str("TEST.1.10").unwrap(),
        mip_store.clone(),
    )
    .unwrap()
    .unwrap();

    // launch the get_state process
    let (mut mock_remote_connector, mut remote_interface) = mock_establisher::new();
    let get_state_h = tokio::spawn(async move {
        get_state(
            bootstrap_config,
            final_state_client_clone,
            mock_remote_connector.get_connector(),
            Version::from_str("TEST.1.10").unwrap(),
            MassaTime::now().unwrap().saturating_sub(1000.into()),
            None,
            None,
        )
        .await
        .unwrap()
    });

    // accept connection attempt from remote
    let remote_bridge = std::thread::spawn(move || {
        let (remote_rw, conn_addr, waker) = remote_interface
            .wait_connection_attempt_from_controller()
            .expect("timeout waiting for connection attempt from remote");
        let expect_conn_addr = bootstrap_config.bootstrap_list[0].0;
        assert_eq!(
            conn_addr, expect_conn_addr,
            "client connected to wrong bootstrap ip"
        );
        waker.store(true, Ordering::Relaxed);
        remote_rw
    });

    // connect to bootstrap
    let remote_addr = std::net::SocketAddr::from_str("82.245.72.98:10000").unwrap(); // not checked
    let bootstrap_bridge = tokio::time::timeout(
        std::time::Duration::from_millis(1000),
        bootstrap_interface.connect_to_controller(&remote_addr),
    )
    .await
    .expect("timeout while connecting to bootstrap")
    .expect("could not connect to bootstrap");

    // launch bridge
    bootstrap_bridge.set_nonblocking(true).unwrap();
    let bootstrap_bridge = TcpStream::from_std(bootstrap_bridge).unwrap();
    let bridge = tokio::spawn(async move {
        let remote_bridge = remote_bridge.join().unwrap();
        remote_bridge.set_nonblocking(true).unwrap();
        let remote_bridge = TcpStream::from_std(remote_bridge).unwrap();
        bridge_mock_streams(remote_bridge, bootstrap_bridge).await;
    });

    // intercept peers being asked
    // TODO: This would ideally be mocked such that the bootstrap server takes an impl of the network controller.
    // and the impl is mocked such that it will just return the sent peers
    let wait_peers = async move || {
        // wait for bootstrap to ask network for peers, send them
        let response =
            match wait_network_command(&mut network_cmd_rx, 20_000.into(), |cmd| match cmd {
                NetworkCommand::GetBootstrapPeers(resp) => Some(resp),
                _ => None,
            })
            .await
            {
                Some(resp) => resp,
                None => panic!("timeout waiting for get peers command"),
            };
        let sent_peers = get_peers();
        response.send(sent_peers.clone()).unwrap();
        sent_peers
    };

    // intercept consensus parts being asked
    let sent_graph = get_boot_state();
    let sent_graph_clone = sent_graph.clone();
    std::thread::spawn(move || loop {
        consensus_event_receiver.wait_command(MassaTime::from_millis(20_000), |cmd| match &cmd {
            MockConsensusControllerMessage::GetBootstrapableGraph {
                execution_cursor,
                response_tx,
                ..
            } => {
                // send the consensus blocks at the 4th slot (1 for startup + 3 for safety)
                // give an empty answer for any other call
                if execution_cursor
                    == &StreamingStep::Ongoing(Slot {
                        period: 1,
                        thread: 1,
                    })
                {
                    response_tx
                        .send(Ok((
                            sent_graph_clone.clone(),
                            PreHashSet::default(),
                            StreamingStep::Started,
                        )))
                        .unwrap();
                } else {
                    response_tx
                        .send(Ok((
                            BootstrapableGraph {
                                final_blocks: Vec::new(),
                            },
                            PreHashSet::default(),
                            StreamingStep::Finished(None),
                        )))
                        .unwrap();
                }
                Some(())
            }
            _ => None,
        });
    });

    // launch the modifier thread
    let list_changes: Arc<RwLock<Vec<(Slot, StateChanges)>>> = Arc::new(RwLock::new(Vec::new()));
    let list_changes_clone = list_changes.clone();
    std::thread::spawn(move || {
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(500));
            let mut final_write = final_state_server_clone.write();
            let next = final_write.slot.get_next_slot(thread_count).unwrap();
            final_write.slot = next;
            let changes = StateChanges {
                pos_changes: get_random_pos_changes(10),
                ledger_changes: get_random_ledger_changes(10),
                async_pool_changes: get_random_async_pool_changes(10),
                executed_ops_changes: get_random_executed_ops_changes(10),
            };
            final_write
                .changes_history
                .push_back((next, changes.clone()));
            let mut list_changes_write = list_changes_clone.write();
            list_changes_write.push((next, changes));
        }
    });

    // wait for peers and graph
    let sent_peers = wait_peers().await;

    // wait for get_state
    let bootstrap_res = get_state_h
        .await
        .expect("error while waiting for get_state to finish");

    // wait for bridge
    bridge.await.expect("bridge join failed");

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
            final_state_server_write
                .ledger
                .apply_changes(change.ledger_changes.clone(), *slot);
            final_state_server_write
                .async_pool
                .apply_changes_unchecked(&change.async_pool_changes);
            final_state_server_write
                .executed_ops
                .apply_changes(change.executed_ops_changes.clone(), *slot);
        }
    }

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
        sent_peers.0,
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
    bootstrap_manager
        .stop()
        .await
        .expect("could not stop bootstrap server");

    // stop selector controllers
    server_selector_manager.stop();
    client_selector_manager.stop();
}
