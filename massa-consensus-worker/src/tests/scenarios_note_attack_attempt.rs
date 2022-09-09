// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::*;
use crate::start_consensus_controller;
use massa_pool_exports::test_exports::MockPoolController;

use massa_consensus_exports::settings::ConsensusChannels;
use massa_consensus_exports::ConsensusConfig;
use massa_execution_exports::test_exports::MockExecutionController;
use massa_hash::Hash;
use massa_models::{address::Address, block::BlockId, slot::Slot};
use massa_pos_exports::SelectorConfig;
use massa_pos_worker::start_selector_worker;
use massa_protocol_exports::test_exports::MockProtocolController;
use massa_signature::KeyPair;
use massa_storage::Storage;
use serial_test::serial;

#[tokio::test]
#[serial]
#[ignore]
async fn test_invalid_block_notified_as_attack_attempt() {
    let staking_keys: Vec<KeyPair> = (0..1).map(|_| KeyPair::generate()).collect();
    let cfg = ConsensusConfig {
        t0: 32.into(),
        future_block_processing_max_periods: 50,
        ..ConsensusConfig::default()
    };

    let storage: Storage = Storage::create_root();

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let selector_config = SelectorConfig {
        thread_count: 2,
        periods_per_cycle: 100,
        genesis_address: Address::from_public_key(&staking_keys[0].get_public_key()),
        endorsement_count: 0,
        max_draw_cache: 10,
        channel_size: 256,
    };
    let (_selector_manager, selector_controller) = start_selector_worker(selector_config).unwrap();
    let (pool_controller, pool_event_receiver) = MockPoolController::new_with_receiver();
    let (execution_controller, _execution_rx) = MockExecutionController::new_with_receiver();
    // launch consensus controller
    let (consensus_command_sender, _consensus_event_receiver, _consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            ConsensusChannels {
                execution_controller,
                protocol_command_sender: protocol_command_sender.clone(),
                protocol_event_receiver,
                pool_command_sender: pool_controller,
                selector_controller,
            },
            None,
            storage.clone(),
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
    let block = create_block_with_merkle_root(
        &cfg,
        Hash::compute_from("different".as_bytes()),
        Slot::new(1, cfg.thread_count + 1),
        parents.clone(),
        &staking_keys[0],
    );
    let block_id = block.id;
    let slot = block.content.header.content.slot;
    protocol_controller
        .receive_block(block_id, slot, storage.clone())
        .await;

    validate_notify_block_attack_attempt(&mut protocol_controller, block_id, 1000).await;
}

#[tokio::test]
#[serial]
#[ignore]
async fn test_invalid_header_notified_as_attack_attempt() {
    let staking_keys: Vec<KeyPair> = (0..1).map(|_| KeyPair::generate()).collect();
    let cfg = ConsensusConfig {
        t0: 32.into(),
        future_block_processing_max_periods: 50,
        ..ConsensusConfig::default()
    };

    // mock protocol & pool
    let (mut protocol_controller, protocol_command_sender, protocol_event_receiver) =
        MockProtocolController::new();
    let (pool_controller, pool_event_receiver) = MockPoolController::new_with_receiver();
    let selector_config = SelectorConfig {
        thread_count: 2,
        periods_per_cycle: 100,
        genesis_address: Address::from_public_key(&staking_keys[0].get_public_key()),
        endorsement_count: 0,
        max_draw_cache: 10,
        channel_size: 256,
    };
    let (_selector_manager, selector_controller) = start_selector_worker(selector_config).unwrap();
    let (execution_controller, _execution_rx) = MockExecutionController::new_with_receiver();
    let storage: Storage = Storage::create_root();
    // launch consensus controller
    let (consensus_command_sender, _consensus_event_receiver, _consensus_manager) =
        start_consensus_controller(
            cfg.clone(),
            ConsensusChannels {
                execution_controller,
                protocol_command_sender: protocol_command_sender.clone(),
                protocol_event_receiver,
                pool_command_sender: pool_controller,
                selector_controller,
            },
            None,
            storage,
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
    let block = create_block_with_merkle_root(
        &cfg,
        Hash::compute_from("different".as_bytes()),
        Slot::new(1, cfg.thread_count + 1),
        parents.clone(),
        &staking_keys[0],
    );
    protocol_controller
        .receive_header(block.content.header)
        .await;

    validate_notify_block_attack_attempt(&mut protocol_controller, block.id, 1000).await;
}
