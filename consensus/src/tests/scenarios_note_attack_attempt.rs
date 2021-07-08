use std::collections::HashMap;

use super::{
    mock_pool_controller::{MockPoolController, PoolCommandSink},
    mock_protocol_controller::MockProtocolController,
    tools,
};
use crate::{start_consensus_controller, tests::tools::generate_ledger_file};
use crypto::hash::Hash;
use models::Slot;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_invalid_block_notified_as_attack_attempt() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let mut cfg = tools::default_consensus_config(1, ledger_file.path());
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, pool_command_sender) = MockPoolController::new();
    let _pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    let parents = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .best_parents;

    // Block for a non-existent thread.
    let (hash, block, _) = tools::create_block_with_merkle_root(
        &cfg,
        Hash::hash("different".as_bytes()),
        Slot::new(1, cfg.thread_count + 1),
        parents.clone(),
        cfg.nodes[0].clone(),
    );
    protocol_controller.receive_block(block).await;

    tools::validate_notify_block_attack_attempt(&mut protocol_controller, hash, 1000).await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
#[serial]
async fn test_invalid_header_notified_as_attack_attempt() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let mut cfg = tools::default_consensus_config(1, ledger_file.path());
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, pool_command_sender) = MockPoolController::new();
    let _pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    let parents = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .best_parents;

    // Block for a non-existent thread.
    let (hash, block, _) = tools::create_block_with_merkle_root(
        &cfg,
        Hash::hash("different".as_bytes()),
        Slot::new(1, cfg.thread_count + 1),
        parents.clone(),
        cfg.nodes[0].clone(),
    );
    protocol_controller.receive_header(block.header).await;

    tools::validate_notify_block_attack_attempt(&mut protocol_controller, hash, 1000).await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}
