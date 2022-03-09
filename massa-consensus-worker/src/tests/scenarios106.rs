// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test scenarios106 -- --nocapture

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;

use massa_models::prehash::Set;
use massa_models::timeslots;
use massa_models::{BlockId, Slot};
use massa_signature::{generate_random_private_key, PrivateKey};
use massa_time::MassaTime;
use serial_test::serial;
use std::collections::HashSet;
use std::time::Duration;

#[tokio::test]
#[serial]
async fn test_unsorted_block() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        future_block_processing_max_periods: 50,
        max_future_processing_blocks: 10,
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let start_period = 3;
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;
            // create test blocks

            let (hasht0s1, t0s1, _) = create_block(
                &cfg,
                Slot::new(1 + start_period, 0),
                genesis_hashes.clone(),
                staking_keys[0],
            );

            let (hasht1s1, t1s1, _) = create_block(
                &cfg,
                Slot::new(1 + start_period, 1),
                genesis_hashes.clone(),
                staking_keys[0],
            );

            let (hasht0s2, t0s2, _) = create_block(
                &cfg,
                Slot::new(2 + start_period, 0),
                vec![hasht0s1, hasht1s1],
                staking_keys[0],
            );
            let (hasht1s2, t1s2, _) = create_block(
                &cfg,
                Slot::new(2 + start_period, 1),
                vec![hasht0s1, hasht1s1],
                staking_keys[0],
            );

            let (hasht0s3, t0s3, _) = create_block(
                &cfg,
                Slot::new(3 + start_period, 0),
                vec![hasht0s2, hasht1s2],
                staking_keys[0],
            );
            let (hasht1s3, t1s3, _) = create_block(
                &cfg,
                Slot::new(3 + start_period, 1),
                vec![hasht0s2, hasht1s2],
                staking_keys[0],
            );

            let (hasht0s4, t0s4, _) = create_block(
                &cfg,
                Slot::new(4 + start_period, 0),
                vec![hasht0s3, hasht1s3],
                staking_keys[0],
            );
            let (hasht1s4, t1s4, _) = create_block(
                &cfg,
                Slot::new(4 + start_period, 1),
                vec![hasht0s3, hasht1s3],
                staking_keys[0],
            );

            // send blocks  t0s1, t1s1,
            protocol_controller.receive_block(t0s1).await;
            protocol_controller.receive_block(t1s1).await;
            // send blocks t0s3, t1s4, t0s4, t0s2, t1s3, t1s2
            protocol_controller.receive_block(t0s3).await;
            protocol_controller.receive_block(t1s4).await;
            protocol_controller.receive_block(t0s4).await;
            protocol_controller.receive_block(t0s2).await;
            protocol_controller.receive_block(t1s3).await;
            protocol_controller.receive_block(t1s2).await;

            // block t0s1 and t1s1 are propagated
            let hash_list = vec![hasht0s1, hasht1s1];
            validate_propagate_block_in_list(
                &mut protocol_controller,
                &hash_list,
                3000 + start_period * 1000,
            )
            .await;
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            // block t0s2 and t1s2 are propagated
            let hash_list = vec![hasht0s2, hasht1s2];
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            // block t0s3 and t1s3 are propagated
            let hash_list = vec![hasht0s3, hasht1s3];
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            // block t0s4 and t1s4 are propagated
            let hash_list = vec![hasht0s4, hasht1s4];
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 4000).await;
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}

//test future_incoming_blocks block in the future with max_future_processing_blocks.
#[tokio::test]
#[serial]
async fn test_unsorted_block_with_to_much_in_the_future() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        // slot 1 is in the past
        genesis_timestamp: MassaTime::now().unwrap().saturating_sub(2000.into()),
        future_block_processing_max_periods: 3,
        max_future_processing_blocks: 5,
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            // create test blocks
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // a block in the past must be propagated
            let (hash1, block1, _) = create_block(
                &cfg,
                Slot::new(1, 0),
                genesis_hashes.clone(),
                staking_keys[0],
            );
            protocol_controller.receive_block(block1).await;
            validate_propagate_block(&mut protocol_controller, hash1, 2500).await;

            // this block is slightly in the future: will wait for it
            let slot = timeslots::get_current_latest_block_slot(
                cfg.thread_count,
                cfg.t0,
                cfg.genesis_timestamp,
                0,
            )
            .unwrap()
            .unwrap();
            let (hash2, block2, _) = create_block(
                &cfg,
                Slot::new(slot.period + 2, slot.thread),
                genesis_hashes.clone(),
                staking_keys[0],
            );
            protocol_controller.receive_block(block2).await;
            assert!(!validate_notpropagate_block(&mut protocol_controller, hash2, 500).await);
            validate_propagate_block(&mut protocol_controller, hash2, 2500).await;

            // this block is too much in the future: do not process
            let slot = timeslots::get_current_latest_block_slot(
                cfg.thread_count,
                cfg.t0,
                cfg.genesis_timestamp,
                0,
            )
            .unwrap()
            .unwrap();
            let (hash3, block3, _) = create_block(
                &cfg,
                Slot::new(slot.period + 1000, slot.thread),
                genesis_hashes.clone(),
                staking_keys[0],
            );
            protocol_controller.receive_block(block3).await;
            assert!(!validate_notpropagate_block(&mut protocol_controller, hash3, 2500).await);

            // Check that the block has been silently dropped and not discarded for being too much in the future.
            let block_graph = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .unwrap();
            assert!(!block_graph.active_blocks.contains_key(&hash3));
            assert!(!block_graph.discarded_blocks.contains_key(&hash3));
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
async fn test_too_many_blocks_in_the_future() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        delta_f0: 1000,
        future_block_processing_max_periods: 100,
        // slot 1 is in the past
        genesis_timestamp: MassaTime::now().unwrap().saturating_sub(2000.into()),
        max_future_processing_blocks: 2,
        t0: 1000.into(),
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            // get genesis block hashes
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // generate 5 blocks but there is only space for 2 in the waiting line
            let mut expected_block_hashes: HashSet<BlockId> = HashSet::new();
            let mut max_period = 0;
            let slot = timeslots::get_current_latest_block_slot(
                cfg.thread_count,
                cfg.t0,
                cfg.genesis_timestamp,
                0,
            )
            .unwrap()
            .unwrap();
            for period in 0..5 {
                max_period = slot.period + 2 + period;
                let (hash, block, _) = create_block(
                    &cfg,
                    Slot::new(max_period, slot.thread),
                    genesis_hashes.clone(),
                    staking_keys[0],
                );
                protocol_controller.receive_block(block).await;
                if period < 2 {
                    expected_block_hashes.insert(hash);
                }
            }
            // wait for the 2 waiting blocks to propagate
            let mut expected_clone = expected_block_hashes.clone();
            while !expected_block_hashes.is_empty() {
                assert!(
                    expected_block_hashes.remove(
                        &validate_propagate_block_in_list(
                            &mut protocol_controller,
                            &expected_block_hashes.iter().copied().collect(),
                            2500
                        )
                        .await
                    ),
                    "unexpected block propagated"
                );
            }
            // wait until we reach the slot of the last block
            while timeslots::get_current_latest_block_slot(
                cfg.thread_count,
                cfg.t0,
                cfg.genesis_timestamp,
                0,
            )
            .unwrap()
            .unwrap()
                < Slot::new(max_period + 1, 0)
            {}
            // ensure that the graph contains only what we expect
            let graph = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status");
            expected_clone.extend(graph.genesis_blocks);
            assert_eq!(
                expected_clone,
                graph
                    .active_blocks
                    .keys()
                    .copied()
                    .collect::<HashSet<BlockId>>(),
                "unexpected block graph"
            );
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
async fn test_dep_in_back_order() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        genesis_timestamp: MassaTime::now()
            .unwrap()
            .saturating_sub(MassaTime::from(1000).checked_mul(1000).unwrap()),
        t0: 1000.into(),
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };
    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // create test blocks
            let (hasht0s1, t0s1, _) = create_block(
                &cfg,
                Slot::new(1, 0),
                genesis_hashes.clone(),
                staking_keys[0],
            );

            let (hasht1s1, t1s1, _) = create_block(
                &cfg,
                Slot::new(1, 1),
                genesis_hashes.clone(),
                staking_keys[0],
            );

            let (hasht0s2, t0s2, _) = create_block(
                &cfg,
                Slot::new(2, 0),
                vec![hasht0s1, hasht1s1],
                staking_keys[0],
            );
            let (hasht1s2, t1s2, _) = create_block(
                &cfg,
                Slot::new(2, 1),
                vec![hasht0s1, hasht1s1],
                staking_keys[0],
            );

            let (hasht0s3, t0s3, _) = create_block(
                &cfg,
                Slot::new(3, 0),
                vec![hasht0s2, hasht1s2],
                staking_keys[0],
            );
            let (hasht1s3, t1s3, _) = create_block(
                &cfg,
                Slot::new(3, 1),
                vec![hasht0s2, hasht1s2],
                staking_keys[0],
            );

            let (hasht0s4, t0s4, _) = create_block(
                &cfg,
                Slot::new(4, 0),
                vec![hasht0s3, hasht1s3],
                staking_keys[0],
            );
            let (hasht1s4, t1s4, _) = create_block(
                &cfg,
                Slot::new(4, 1),
                vec![hasht0s3, hasht1s3],
                staking_keys[0],
            );

            // send blocks   t0s2, t1s3, t0s1, t0s4, t1s4, t1s1, t0s3, t1s2
            protocol_controller.receive_block(t0s2).await; // not propagated and update wishlist
            validate_wishlist(
                &mut protocol_controller,
                vec![hasht0s1, hasht1s1].into_iter().collect(),
                Set::<BlockId>::default(),
                500,
            )
            .await;
            validate_notpropagate_block(&mut protocol_controller, hasht0s2, 500).await;

            protocol_controller.receive_block(t1s3).await; // not propagated and no wishlist update
            validate_notpropagate_block(&mut protocol_controller, hasht1s3, 500).await;

            protocol_controller.receive_block(t0s1).await; // we have its parents so it should be integrated right now and update wishlist

            validate_propagate_block(&mut protocol_controller, hasht0s1, 500).await;
            validate_wishlist(
                &mut protocol_controller,
                Set::<BlockId>::default(),
                vec![hasht0s1].into_iter().collect(),
                500,
            )
            .await;

            protocol_controller.receive_block(t0s4).await; // not propagated and no wishlist update
            validate_notpropagate_block(&mut protocol_controller, hasht0s4, 500).await;

            protocol_controller.receive_block(t1s4).await; // not propagated and no wishlist update
            validate_notpropagate_block(&mut protocol_controller, hasht1s4, 500).await;

            protocol_controller.receive_block(t1s1).await; // assert t1s1 is integrated and t0s2 is integrated and wishlist updated
            validate_propagate_block_in_list(
                &mut protocol_controller,
                &vec![hasht1s1, hasht0s2],
                500,
            )
            .await;

            validate_propagate_block_in_list(
                &mut protocol_controller,
                &vec![hasht1s1, hasht0s2],
                500,
            )
            .await;
            validate_wishlist(
                &mut protocol_controller,
                vec![].into_iter().collect(),
                vec![hasht1s1].into_iter().collect(),
                500,
            )
            .await;

            protocol_controller.receive_block(t0s3).await; // not propagated and no wishlist update
            validate_notpropagate_block(&mut protocol_controller, hasht0s3, 500).await;

            protocol_controller.receive_block(t1s2).await;

            // All remaining blocks are propagated
            let integrated = vec![hasht1s2, hasht0s3, hasht1s3, hasht0s4, hasht1s4];
            validate_propagate_block_in_list(&mut protocol_controller, &integrated, 1000).await;
            validate_propagate_block_in_list(&mut protocol_controller, &integrated, 1000).await;
            validate_propagate_block_in_list(&mut protocol_controller, &integrated, 1000).await;
            validate_propagate_block_in_list(&mut protocol_controller, &integrated, 1000).await;
            validate_propagate_block_in_list(&mut protocol_controller, &integrated, 1000).await;
            validate_wishlist(
                &mut protocol_controller,
                Set::<BlockId>::default(),
                vec![hasht1s2].into_iter().collect(),
                500,
            )
            .await;
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
async fn test_dep_in_back_order_with_max_dependency_blocks() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        genesis_timestamp: MassaTime::now()
            .unwrap()
            .saturating_sub(MassaTime::from(1000).checked_mul(1000).unwrap()),
        max_dependency_blocks: 2,
        t0: 1000.into(),
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };
    tokio::time::sleep(Duration::from_millis(1000)).await;

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // create test blocks

            let (hasht0s1, t0s1, _) = create_block(
                &cfg,
                Slot::new(1, 0),
                genesis_hashes.clone(),
                staking_keys[0],
            );

            let (hasht1s1, t1s1, _) = create_block(
                &cfg,
                Slot::new(1, 1),
                genesis_hashes.clone(),
                staking_keys[0],
            );

            let (hasht0s2, t0s2, _) = create_block(
                &cfg,
                Slot::new(2, 0),
                vec![hasht0s1, hasht1s1],
                staking_keys[0],
            );
            let (hasht1s2, t1s2, _) = create_block(
                &cfg,
                Slot::new(2, 1),
                vec![hasht0s1, hasht1s1],
                staking_keys[0],
            );

            let (hasht0s3, t0s3, _) = create_block(
                &cfg,
                Slot::new(3, 0),
                vec![hasht0s2, hasht1s2],
                staking_keys[0],
            );
            let (hasht1s3, t1s3, _) = create_block(
                &cfg,
                Slot::new(3, 1),
                vec![hasht0s2, hasht1s2],
                staking_keys[0],
            );

            // send blocks   t0s2, t1s3, t0s1, t0s4, t1s4, t1s1, t0s3, t1s2
            protocol_controller.receive_block(t0s2).await;
            validate_wishlist(
                &mut protocol_controller,
                vec![hasht0s1, hasht1s1].into_iter().collect(),
                Set::<BlockId>::default(),
                500,
            )
            .await;
            validate_notpropagate_block(&mut protocol_controller, hasht0s2, 500).await;

            protocol_controller.receive_block(t1s3).await;
            validate_notpropagate_block(&mut protocol_controller, hasht1s3, 500).await;

            protocol_controller.receive_block(t0s1).await;
            validate_propagate_block(&mut protocol_controller, hasht0s1, 500).await;
            validate_wishlist(
                &mut protocol_controller,
                Set::<BlockId>::default(),
                vec![hasht0s1].into_iter().collect(),
                500,
            )
            .await;
            protocol_controller.receive_block(t0s3).await;
            validate_notpropagate_block(&mut protocol_controller, hasht0s3, 500).await;

            protocol_controller.receive_block(t1s2).await;
            validate_notpropagate_block(&mut protocol_controller, hasht1s2, 500).await;

            protocol_controller.receive_block(t1s1).await;
            validate_propagate_block_in_list(
                &mut protocol_controller,
                &vec![hasht1s1, hasht1s2],
                500,
            )
            .await;
            validate_propagate_block_in_list(
                &mut protocol_controller,
                &vec![hasht1s1, hasht1s2],
                500,
            )
            .await;
            validate_wishlist(
                &mut protocol_controller,
                Set::<BlockId>::default(),
                vec![hasht1s1].into_iter().collect(),
                500,
            )
            .await;

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
async fn test_add_block_that_depends_on_invalid_block() {
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        genesis_timestamp: MassaTime::now()
            .unwrap()
            .saturating_sub(MassaTime::from(1000).checked_mul(1000).unwrap()),
        max_dependency_blocks: 7,
        t0: 1000.into(),
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // create test blocks
            let (hasht0s1, t0s1, _) = create_block(
                &cfg,
                Slot::new(1, 0),
                genesis_hashes.clone(),
                staking_keys[0],
            );

            let (hasht1s1, t1s1, _) = create_block(
                &cfg,
                Slot::new(1, 1),
                genesis_hashes.clone(),
                staking_keys[0],
            );

            // blocks t3s2 with wrong thread and (t0s1, t1s1) parents.
            let (hasht3s2, t3s2, _) = create_block(
                &cfg,
                Slot::new(2, 3),
                vec![hasht0s1, hasht1s1],
                staking_keys[0],
            );

            // blocks t0s3 and t1s3 with (t3s2, t1s2) parents.
            let (hasht0s3, t0s3, _) = create_block(
                &cfg,
                Slot::new(3, 0),
                vec![hasht3s2, hasht1s1],
                staking_keys[0],
            );
            let (hasht1s3, t1s3, _) = create_block(
                &cfg,
                Slot::new(3, 1),
                vec![hasht3s2, hasht1s1],
                staking_keys[0],
            );

            // add block in this order t0s1, t1s1, t0s3, t1s3, t3s2
            // send blocks   t0s2, t1s3, t0s1, t0s4, t1s4, t1s1, t0s3, t1s2
            protocol_controller.receive_block(t0s1).await;
            protocol_controller.receive_block(t1s1).await;
            protocol_controller.receive_block(t0s3).await;
            protocol_controller.receive_block(t1s3).await;
            protocol_controller.receive_block(t3s2).await;

            // block t0s1 and t1s1 are propagated
            let hash_list = vec![hasht0s1, hasht1s1];
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;
            validate_propagate_block_in_list(&mut protocol_controller, &hash_list, 1000).await;

            // block  t0s3, t1s3 are not propagated
            let hash_list = vec![hasht0s3, hasht1s3];
            assert!(
                !validate_notpropagate_block_in_list(&mut protocol_controller, &hash_list, 2000)
                    .await
            );
            assert!(
                !validate_notpropagate_block_in_list(&mut protocol_controller, &hash_list, 2000)
                    .await
            );
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
