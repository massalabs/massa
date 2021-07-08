//RUST_BACKTRACE=1 cargo test scenarios106 -- --nocapture

use super::{mock_protocol_controller::MockProtocolController, tools};
use crate::start_consensus_controller;
use models::Slot;

#[tokio::test]
async fn test_consensus_asks_for_block() {
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
        )
        .await
        .expect("could not start consensus controller");

    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    //create test blocks
    let (hasht0s1, t0s1, _) = tools::create_block(
        &cfg,
        &serialization_context,
        Slot::new(1, 0),
        genesis_hashes.clone(),
    );
    //send header for block t0s1
    protocol_controller
        .receive_header(t0s1.header.clone())
        .await;

    tools::validate_ask_for_block(&mut protocol_controller, hasht0s1, 1000).await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_consensus_does_not_ask_for_block() {
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
        )
        .await
        .expect("could not start consensus controller");

    let start_slot = 3;
    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    //create test blocks
    let (hasht0s1, t0s1, _) = tools::create_block(
        &cfg,
        &serialization_context,
        Slot::new(1 + start_slot, 0),
        genesis_hashes.clone(),
    );
    let header = t0s1.header.clone();

    // Send the actual block.
    protocol_controller.receive_block(t0s1).await;

    //block t0s1 is propagated
    let hash_list = vec![hasht0s1];
    tools::validate_propagate_block_in_list(
        &mut protocol_controller,
        &hash_list,
        3000 + start_slot as u64 * 1000,
    )
    .await;

    // Send the hash
    protocol_controller.receive_header(header).await;

    // Consensus should not ask for the block, so the time-out should be hit.
    tools::validate_does_not_ask_for_block(&mut protocol_controller, &hasht0s1, 10).await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}
