use super::{mock_protocol_controller::MockProtocolController, tools};
use crate::start_consensus_controller;
use crypto::hash::Hash;
use models::Slot;

#[tokio::test]
async fn test_invalid_block_notified_as_attack_attempt() {
    let (mut cfg, serialization_context) = tools::default_consensus_config(1);
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new(serialization_context.clone());

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            serialization_context.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            None,
            None,
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
        &serialization_context,
        Hash::hash("different".as_bytes()),
        Slot::new(1, cfg.thread_count + 1),
        parents.clone(),
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
async fn test_invalid_header_notified_as_attack_attempt() {
    let (mut cfg, serialization_context) = tools::default_consensus_config(1);
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new(serialization_context.clone());

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            serialization_context.clone(),
            protocol_command_sender.clone(),
            protocol_event_receiver,
            None,
            None,
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
        &serialization_context,
        Hash::hash("different".as_bytes()),
        Slot::new(1, cfg.thread_count + 1),
        parents.clone(),
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
