// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::{
    mock_execution_controller::MockExecutionController,
    mock_pool_controller::{MockPoolController, PoolCommandSink},
    mock_protocol_controller::MockProtocolController,
    tools,
};
use crate::{start_consensus_controller, tests::tools::generate_ledger_file};
use massa_hash::hash::Hash;
use massa_models::{BlockId, Slot};
use massa_signature::{generate_random_private_key, PrivateKey};
use serial_test::serial;
use std::collections::HashMap;

#[tokio::test]
#[serial]
async fn test_invalid_block_notified_as_attack_attempt() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, pool_command_sender) = MockPoolController::new();
    let pool_sink = PoolCommandSink::new(pool_controller).await;
    let (mut _execution_controller, execution_command_sender, execution_event_receiver) =
        MockExecutionController::new();

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            execution_command_sender,
            execution_event_receiver,
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    let parents: Vec<BlockId> = consensus_command_sender
        .get_block_graph_status(None, None)
        .await
        .expect("could not get block graph status")
        .best_parents
        .iter()
        .map(|(b, _p)| *b)
        .collect();

    // Block for a non-existent thread.
    let (hash, block, _) = tools::create_block_with_merkle_root(
        &cfg,
        Hash::compute_from("different".as_bytes()),
        Slot::new(1, cfg.thread_count + 1),
        parents.clone(),
        staking_keys[0],
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
    pool_sink.stop().await;
}

#[tokio::test]
#[serial]
async fn test_invalid_header_notified_as_attack_attempt() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, pool_command_sender) = MockPoolController::new();
    let (mut _execution_controller, execution_command_sender, execution_event_receiver) =
        MockExecutionController::new();
    let pool_sink = PoolCommandSink::new(pool_controller).await;

    // launch consensus controller
    let (consensus_command_sender, consensus_event_receiver, consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            execution_command_sender,
            execution_event_receiver,
            protocol_command_sender.clone(),
            protocol_event_receiver,
            pool_command_sender,
            None,
            None,
            0,
        )
        .await
        .expect("could not start consensus controller");

    let parents: Vec<BlockId> = consensus_command_sender
        .get_block_graph_status(None, None)
        .await
        .expect("could not get block graph status")
        .best_parents
        .iter()
        .map(|(b, _p)| *b)
        .collect();

    // Block for a non-existent thread.
    let (hash, block, _) = tools::create_block_with_merkle_root(
        &cfg,
        Hash::compute_from("different".as_bytes()),
        Slot::new(1, cfg.thread_count + 1),
        parents.clone(),
        staking_keys[0],
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
    pool_sink.stop().await;
}
