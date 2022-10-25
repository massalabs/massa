// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::{
    mock_establisher,
    tools::{
        bridge_mock_streams, get_boot_state, get_peers, get_random_final_state_bootstrap,
        get_random_ledger_changes, wait_network_command,
    },
};
use crate::tests::tools::{
    get_random_async_pool_changes, get_random_executed_ops, get_random_pos_changes,
};
use crate::BootstrapConfig;
use crate::{
    get_state, start_bootstrap_server,
    tests::tools::{assert_eq_bootstrap_graph, get_bootstrap_config},
};
use massa_consensus_exports::test_exports::{MockConsensusController, MockConsensusControllerMessage};
use massa_final_state::{test_exports::assert_eq_final_state, FinalState, StateChanges};
use massa_models::{address::Address, slot::Slot, version::Version};
use massa_network_exports::{NetworkCommand, NetworkCommandSender};
use massa_pos_exports::{test_exports::assert_eq_pos_selection, PoSFinalState, SelectorConfig};
use massa_pos_worker::start_selector_worker;
use massa_signature::KeyPair;
use massa_time::MassaTime;
use parking_lot::RwLock;
use serial_test::serial;
use std::{path::PathBuf, str::FromStr, sync::Arc, time::Duration};
use tokio::sync::mpsc;

lazy_static::lazy_static! {
    pub static ref BOOTSTRAP_CONFIG_KEYPAIR: (BootstrapConfig, KeyPair) = {
        let keypair = KeyPair::generate();
        (get_bootstrap_config(keypair.get_public_key()), keypair)
    };
}

#[tokio::test]
#[serial]
async fn test_bootstrap_server() {
    let (bootstrap_config, keypair): &(BootstrapConfig, KeyPair) = &BOOTSTRAP_CONFIG_KEYPAIR;

    let rolls_path = PathBuf::from_str("../massa-node/base_config/initial_rolls.json").unwrap();
    let genesis_address = Address::from_public_key(&KeyPair::generate().get_public_key());
    let (mut server_selector_manager, server_selector_controller) =
        start_selector_worker(SelectorConfig {
            thread_count: 2,
            periods_per_cycle: 2,
            genesis_address,
            ..Default::default()
        })
        .expect("could not start server selector controller");
    let (mut client_selector_manager, client_selector_controller) =
        start_selector_worker(SelectorConfig {
            thread_count: 2,
            periods_per_cycle: 2,
            genesis_address,
            ..Default::default()
        })
        .expect("could not start client selector controller");

    let (consensus_controller, mut consensus_event_receiver) = MockConsensusController::new_with_receiver();
    let (network_cmd_tx, mut network_cmd_rx) = mpsc::channel::<NetworkCommand>(5);
    let final_state_bootstrap = get_random_final_state_bootstrap(
        PoSFinalState::new(
            &"".to_string(),
            &rolls_path,
            2,
            2,
            server_selector_controller.clone(),
        )
        .unwrap(),
    );
    let final_state = Arc::new(RwLock::new(final_state_bootstrap));

    let (bootstrap_establisher, bootstrap_interface) = mock_establisher::new();
    let bootstrap_manager = start_bootstrap_server(
        consensus_controller,
        NetworkCommandSender(network_cmd_tx),
        final_state.clone(),
        bootstrap_config.clone(),
        bootstrap_establisher,
        keypair.clone(),
        0,
        Version::from_str("TEST.1.10").unwrap(),
    )
    .await
    .unwrap()
    .unwrap();

    let final_state_client = Arc::new(RwLock::new(FinalState::default_with_pos(
        PoSFinalState::new(
            &"".to_string(),
            &rolls_path,
            2,
            2,
            client_selector_controller.clone(),
        )
        .unwrap(),
    )));
    let final_state_client_clone = final_state_client.clone();
    let final_state_clone = final_state.clone();

    // launch the get_state process
    let (remote_establisher, mut remote_interface) = mock_establisher::new();
    let get_state_h = tokio::spawn(async move {
        get_state(
            bootstrap_config,
            final_state_client_clone,
            remote_establisher,
            Version::from_str("TEST.1.10").unwrap(),
            MassaTime::now(0).unwrap().saturating_sub(1000.into()),
            None,
        )
        .await
        .unwrap()
    });

    // accept connection attempt from remote
    let (remote_rw, conn_addr, resp) = tokio::time::timeout(
        std::time::Duration::from_millis(1000),
        remote_interface.wait_connection_attempt_from_controller(),
    )
    .await
    .expect("timeout waiting for connection attempt from remote")
    .expect("error receiving connection attempt from remote");
    let expect_conn_addr = bootstrap_config.bootstrap_list[0].0;
    assert_eq!(
        conn_addr, expect_conn_addr,
        "client connected to wrong bootstrap ip"
    );
    resp.send(true)
        .expect("could not send connection accept to remote");

    // connect to bootstrap
    let remote_addr = std::net::SocketAddr::from_str("82.245.72.98:10000").unwrap(); // not checked
    let bootstrap_rw = tokio::time::timeout(
        std::time::Duration::from_millis(1000),
        bootstrap_interface.connect_to_controller(&remote_addr),
    )
    .await
    .expect("timeout while connecting to bootstrap")
    .expect("could not connect to bootstrap");

    // launch bridge
    let bridge = tokio::spawn(async move {
        bridge_mock_streams(remote_rw, bootstrap_rw).await;
    });

    // intercept peers being asked
    let wait_peers = async move || {
        // wait for bootstrap to ask network for peers, send them
        let response =
            match wait_network_command(&mut network_cmd_rx, 10000.into(), |cmd| match cmd {
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

    // launch the modifier thread
    let list_changes: Arc<RwLock<Vec<(Slot, StateChanges)>>> = Arc::new(RwLock::new(Vec::new()));
    let list_changes_clone = list_changes.clone();
    std::thread::spawn(move || {
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(500));
            let mut final_write = final_state_clone.write();
            let next = final_write.slot.get_next_slot(2).unwrap();
            final_write.slot = next;
            let changes = StateChanges {
                pos_changes: get_random_pos_changes(10),
                ledger_changes: get_random_ledger_changes(10),
                async_pool_changes: get_random_async_pool_changes(10),
                executed_ops: get_random_executed_ops(10),
            };
            final_write
                .changes_history
                .push_back((next, changes.clone()));
            let mut list_changes_write = list_changes_clone.write();
            list_changes_write.push((next, changes));
        }
    });

    let sent_peers = wait_peers().await;
    // wait for peers and graph
    let sent_graph = tokio::task::spawn_blocking(move || {
        let response =
            consensus_event_receiver.wait_command(MassaTime::from_millis(10000), |cmd| match cmd {
                MockConsensusControllerMessage::GetBootstrapableGraph { response_tx } => {
                    let sent_graph = get_boot_state();
                    response_tx.send(Ok(sent_graph.clone())).unwrap();
                    Some(sent_graph)
                }
                _ => panic!("bad command for get boot graph consensus command"),
            });
        match response {
            Some(graph) => graph,
            None => panic!("error waiting for get boot graph consensus command"),
        }
    })
    .await
    .unwrap();

    // wait for get_state
    let bootstrap_res = get_state_h
        .await
        .expect("error while waiting for get_state to finish");

    // wait for bridge
    bridge.await.expect("bridge join failed");

    // apply the changes to the server state before matching with the client
    {
        let mut final_state_write = final_state.write();
        let list_changes_read = list_changes.read().clone();
        // note: skip the first change to match the update loop behaviour
        for (slot, change) in list_changes_read.iter().skip(1) {
            final_state_write
                .pos_state
                .apply_changes(change.pos_changes.clone(), *slot, false)
                .unwrap();
            final_state_write
                .ledger
                .apply_changes(change.ledger_changes.clone(), *slot);
            final_state_write
                .async_pool
                .apply_changes_unchecked(&change.async_pool_changes);
            final_state_write
                .executed_ops
                .extend(change.executed_ops.clone());
        }
    }

    // check final states
    assert_eq_final_state(&final_state.read(), &final_state_client.read());

    // compute initial draws
    final_state.write().compute_initial_draws().unwrap();
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

    // check states
    assert_eq_bootstrap_graph(&sent_graph, &bootstrap_res.graph.unwrap());

    // stop bootstrap server
    bootstrap_manager
        .stop()
        .await
        .expect("could not stop bootstrap server");

    // stop selector controllers
    server_selector_manager.stop();
    client_selector_manager.stop();
}
