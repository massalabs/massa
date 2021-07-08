use super::{mock_protocol_controller::MockProtocolController, tools};
use crate::start_consensus_controller;
use models::Slot;

#[tokio::test]
async fn test_pruning_of_discarded_blocks() {
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

    // Send more bad blocks than the max number of cached discarded.
    for i in 0..(cfg.max_discarded_blocks + 5) as u64 {
        // Too far into the future.
        let _ = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            &serialization_context,
            Slot::new(100000000 + i, 0),
            parents.clone(),
            false,
            false,
        )
        .await;
    }

    let status = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");
    assert!(status.discarded_blocks.map.len() <= cfg.max_discarded_blocks);

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_pruning_of_awaiting_slot_blocks() {
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

    // Send more blocks in the future than the max number of future processinb blocks.
    for i in 0..(cfg.max_future_processing_blocks + 5) as u64 {
        // Too far into the future.
        let _ = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            &serialization_context,
            Slot::new(10 + i, 0),
            parents.clone(),
            false,
            false,
        )
        .await;
    }

    let status = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");
    assert!(status.discarded_blocks.map.len() <= cfg.max_future_processing_blocks);

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_pruning_of_awaiting_dependencies_blocks_with_discarded_dependency() {
    let (mut cfg, serialization_context) = tools::default_consensus_config(1);
    cfg.t0 = 200.into();
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

    // Too far into the future.
    let (bad_parent, bad_block, _) = tools::create_block(
        &cfg,
        &serialization_context,
        Slot::new(10000, 0),
        parents.clone(),
    );

    for i in 1..4 {
        // Sent several headers with the bad parent as dependency.
        let _ = tools::create_and_test_block(
            &mut protocol_controller,
            &cfg,
            &serialization_context,
            Slot::new(i, 0),
            vec![bad_parent.clone(), parents.clone()[0]],
            false,
            false,
        )
        .await;
    }

    // Now, send the bad parent.
    protocol_controller.receive_header(bad_block.header).await;
    tools::validate_notpropagate_block_in_list(&mut protocol_controller, &vec![bad_parent], 10)
        .await;

    // Eventually, all blocks will be discarded due to their bad parent.
    // Note the parent too much in the future will not be discared, but ignored.
    loop {
        let status = consensus_command_sender
            .get_block_graph_status()
            .await
            .expect("could not get block graph status");
        if status.discarded_blocks.map.len() == 3 {
            break;
        }
    }

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}
