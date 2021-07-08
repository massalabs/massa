use std::collections::HashMap;

use super::{
    mock_pool_controller::{MockPoolController, PoolCommandSink},
    mock_protocol_controller::MockProtocolController,
    tools,
};
use crate::{start_consensus_controller, tests::tools::generate_ledger_file};
use models::Slot;

#[tokio::test]
async fn test_parent_in_the_future() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (mut cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
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

    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // Parent, in the future.
    let (hasht0s1, _, _) = tools::create_block(
        &cfg,
        &serialization_context,
        Slot::new(4, 0),
        genesis_hashes.clone(),
    );

    let _ = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(5, 0),
        vec![hasht0s1],
        false,
        false,
    )
    .await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_parents() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (mut cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
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

    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // generate two normal blocks in each thread
    let hasht1s1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(1, 0),
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    let _ = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(1, 1),
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    let _ = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(3, 0),
        vec![hasht1s1, genesis_hashes[0].clone()],
        false,
        false,
    )
    .await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_parents_in_incompatible_cliques() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let (mut cfg, serialization_context) = tools::default_consensus_config(1, ledger_file.path());
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

    let genesis_hashes = consensus_command_sender
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    let hasht0s1 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(1, 0),
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    let hasht0s2 = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(2, 0),
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    // from that point we have two incompatible clique

    let _ = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(1, 1),
        vec![hasht0s1, genesis_hashes[1]],
        true,
        false,
    )
    .await;

    // Block with incompatible parents.
    let _ = tools::create_and_test_block(
        &mut protocol_controller,
        &cfg,
        &serialization_context,
        Slot::new(2, 1),
        vec![hasht0s1, hasht0s2],
        false,
        false,
    )
    .await;

    // stop controller while ignoring all commands
    let stop_fut = consensus_manager.stop(consensus_event_receiver);
    tokio::pin!(stop_fut);
    protocol_controller
        .ignore_commands_while(stop_fut)
        .await
        .unwrap();
}
