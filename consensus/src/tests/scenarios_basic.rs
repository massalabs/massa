// Copyright (c) 2021 MASSA LABS <info@massa.net>

use std::collections::HashMap;

use super::tools;
use crate::tests::tools::generate_ledger_file;
use crypto::hash::Hash;
use models::{BlockId, Slot};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_old_stale_not_propagated_and_discarded() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..1)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    tools::consensus_without_pool_test(
        cfg.clone(),
        None,
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status()
                .await
                .expect("could not get block graph status")
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            let hash_1 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 0),
                parents.clone(),
                true,
                false,
                staking_keys[0].clone(),
            )
            .await;

            let _ = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 1),
                parents.clone(),
                true,
                false,
                staking_keys[0].clone(),
            )
            .await;

            // Old stale block is not propagated.
            let hash_3 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 0),
                vec![hash_1, parents[0]],
                false,
                false,
                staking_keys[0].clone(),
            )
            .await;

            // Old stale block was discarded.
            let status = consensus_command_sender
                .get_block_graph_status()
                .await
                .expect("could not get block graph status");
            assert_eq!(status.discarded_blocks.map.len(), 1);
            assert!(status.discarded_blocks.map.get(&hash_3).is_some());
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_block_not_processed_multiple_times() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..1)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 500.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    tools::consensus_without_pool_test(
        cfg.clone(),
        None,
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status()
                .await
                .expect("could not get block graph status")
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            let (hash_1, block_1, _) = tools::create_block(
                &cfg,
                Slot::new(1, 0),
                parents.clone(),
                staking_keys[0].clone(),
            );
            protocol_controller.receive_block(block_1.clone()).await;
            tools::validate_propagate_block_in_list(
                &mut protocol_controller,
                &vec![hash_1.clone()],
                1000,
            )
            .await;

            // Send it again, it should not be propagated.
            protocol_controller.receive_block(block_1.clone()).await;
            tools::validate_notpropagate_block_in_list(
                &mut protocol_controller,
                &vec![hash_1],
                1000,
            )
            .await;

            // Send it again, it should not be propagated.
            protocol_controller.receive_block(block_1).await;
            tools::validate_notpropagate_block_in_list(
                &mut protocol_controller,
                &vec![hash_1],
                1000,
            )
            .await;

            // Block was not discarded.
            let status = consensus_command_sender
                .get_block_graph_status()
                .await
                .expect("could not get block graph status");
            assert_eq!(status.discarded_blocks.map.len(), 0);
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_queuing() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..1)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    tools::consensus_without_pool_test(
        cfg.clone(),
        None,
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status()
                .await
                .expect("could not get block graph status")
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            // create a block that will be a missing dependency
            let hash_1 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(3, 0),
                parents.clone(),
                false,
                false,
                staking_keys[0].clone(),
            )
            .await;

            // create a block that depends on the missing dep
            let _ = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(4, 0),
                vec![hash_1.clone(), parents[1]],
                false,
                false,
                staking_keys[0].clone(),
            )
            .await;

            // Blocks were queued, not discarded.
            let status = consensus_command_sender
                .get_block_graph_status()
                .await
                .expect("could not get block graph status");
            assert_eq!(status.discarded_blocks.map.len(), 0);
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_double_staking_does_not_propagate() {
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..1)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 1000.into();
    cfg.future_block_processing_max_periods = 50;
    cfg.max_future_processing_blocks = 10;

    tools::consensus_without_pool_test(
        cfg.clone(),
        None,
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status()
                .await
                .expect("could not get block graph status")
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            let _ = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 0),
                parents.clone(),
                true,
                false,
                staking_keys[0].clone(),
            )
            .await;

            // Same creator, same slot, different block
            let (hash_2, block_2, _) = tools::create_block_with_merkle_root(
                &cfg,
                Hash::hash("different".as_bytes()),
                Slot::new(1, 0),
                parents.clone(),
                staking_keys[0].clone(),
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
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
