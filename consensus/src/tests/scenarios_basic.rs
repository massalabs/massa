use super::{
    mock_pool_controller::{MockPoolController, PoolCommandSink},
    mock_protocol_controller::MockProtocolController,
    tools,
};
use crate::start_consensus_controller;
use crypto::hash::Hash;
use models::Slot;

#[tokio::test]
async fn test_old_stale_not_propagated_and_discarded() {
    let (mut cfg, serialization_context) = tools::default_consensus_config(1);
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new(serialization_context.clone());
    let (pool_controller, pool_command_sender) =
        MockPoolController::new(serialization_context.clone());
    let _pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            serialization_context.clone(),
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

    let hash_1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(1, 0),
        parents.clone(),
        true,
        false,
    )
    .await;

    let _ = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(1, 1),
        parents.clone(),
        true,
        false,
    )
    .await;

    // Old stale block is not propagated.
    let hash_3 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(1, 0),
        vec![hash_1, parents[0]],
        false,
        false,
    )
    .await;

    // Old stale block was discarded.
    let status = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");
    assert_eq!(status.discarded_blocks.map.len(), 1);
    assert!(status.discarded_blocks.map.get(&hash_3).is_some());

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_block_not_processed_multiple_times() {
    let (mut cfg, serialization_context) = tools::default_consensus_config(1);
    cfg.t0 = 500.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new(serialization_context.clone());
    let (pool_controller, pool_command_sender) =
        MockPoolController::new(serialization_context.clone());
    let _pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            serialization_context.clone(),
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

    let (hash_1, block_1, _) = tools::create_block(
        &cfg,
        &serialization_context,
        Slot::new(1, 0),
        parents.clone(),
    );
    protocol_controller.receive_block(block_1.clone()).await;
    tools::validate_propagate_block_in_list(&mut protocol_controller, &vec![hash_1.clone()], 1000)
        .await;

    // Send it again, it should not be propagated.
    protocol_controller.receive_block(block_1.clone()).await;
    tools::validate_notpropagate_block_in_list(&mut protocol_controller, &vec![hash_1], 1000).await;

    // Send it again, it should not be propagated.
    protocol_controller.receive_block(block_1).await;
    tools::validate_notpropagate_block_in_list(&mut protocol_controller, &vec![hash_1], 1000).await;

    // Block was not discarded.
    let status = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");
    assert_eq!(status.discarded_blocks.map.len(), 0);

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_queuing() {
    let (mut cfg, serialization_context) = tools::default_consensus_config(1);
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new(serialization_context.clone());
    let (pool_controller, pool_command_sender) =
        MockPoolController::new(serialization_context.clone());
    let _pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            serialization_context.clone(),
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

    // create a block that will be a missing dependency
    let hash_1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(3, 0),
        parents.clone(),
        false,
        false,
    )
    .await;

    // create a block that depends on the missing dep
    let _ = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(4, 0),
        vec![hash_1.clone(), parents[1]],
        false,
        false,
    )
    .await;

    // Blocks were queued, not discarded.
    let status = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");
    assert_eq!(status.discarded_blocks.map.len(), 0);

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_double_staking_does_not_propagate() {
    let (mut cfg, serialization_context) = tools::default_consensus_config(1);
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new(serialization_context.clone());
    let (pool_controller, pool_command_sender) =
        MockPoolController::new(serialization_context.clone());
    let _pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            serialization_context.clone(),
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

    let _ = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(1, 0),
        parents.clone(),
        true,
        false,
    )
    .await;

    // Same creator, same slot, different block
    let (hash_2, block_2, _) = tools::create_block_with_merkle_root(
        &cfg,
        &serialization_context,
        Hash::hash("different".as_bytes()),
        Slot::new(1, 0),
        parents.clone(),
    );
    protocol_controller.receive_block(block_2).await;

    // Note: currently does propagate, see #190.
    tools::validate_propagate_block(&mut protocol_controller, hash_2, 1000).await;

    // Block was not discarded.
    let status = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status");
    assert_eq!(status.discarded_blocks.map.len(), 0);

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}
