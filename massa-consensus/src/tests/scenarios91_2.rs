// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::tests::tools::{self, generate_ledger_file};
use massa_hash::hash::Hash;
use massa_models::Slot;
use massa_signature::{generate_random_private_key, PrivateKey};
use massa_time::MassaTime;
use serial_test::serial;
use std::collections::HashMap;

#[tokio::test]
#[serial]
async fn test_queueing() {
    // setup logging
    // stderrlog::new()
    //     .verbosity(3)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();

    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 32;

    // to avoid timing problems for block in the future
    cfg.genesis_timestamp = MassaTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    tools::consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // * create 30 normal blocks in each thread: in slot 1 they have genesis parents, in slot 2 they have slot 1 parents
            // create a valid block for slot 1
            let mut valid_hasht0 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 0),
                genesis_hashes.clone(),
                true,
                false,
                staking_keys[0],
            )
            .await;

            // create a valid block on the other thread.
            let mut valid_hasht1 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 1),
                genesis_hashes.clone(),
                true,
                false,
                staking_keys[0],
            )
            .await;

            // and loop for the 29 other blocks
            for i in 0..29 {
                valid_hasht0 = tools::create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(i + 2, 0),
                    vec![valid_hasht0, valid_hasht1],
                    true,
                    false,
                    staking_keys[0],
                )
                .await;

                // create a valid block on the other thread.
                valid_hasht1 = tools::create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(i + 2, 1),
                    vec![valid_hasht0, valid_hasht1],
                    true,
                    false,
                    staking_keys[0],
                )
                .await;
            }

            let (missed_hash, _missed_block, _missed_key) = tools::create_block(
                &cfg,
                Slot::new(32, 0),
                vec![valid_hasht0, valid_hasht1],
                staking_keys[0],
            );

            // create 1 block in thread 0 slot 33 with missed block as parent
            valid_hasht0 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(33, 0),
                vec![missed_hash, valid_hasht1],
                false,
                false,
                staking_keys[0],
            )
            .await;

            // and loop again for the 99 other blocks
            for i in 0..30 {
                valid_hasht0 = tools::create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(i + 34, 0),
                    vec![valid_hasht0, valid_hasht1],
                    false,
                    false,
                    staking_keys[0],
                )
                .await;

                // create a valid block on the other thread.
                valid_hasht1 = tools::create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(i + 34, 1),
                    vec![valid_hasht0, valid_hasht1],
                    false,
                    false,
                    staking_keys[0],
                )
                .await;
            }
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
async fn test_doubles() {
    // setup logging
    // stderrlog::new()
    //     .verbosity(3)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();

    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 32;

    // to avoid timing pb for block in the future
    cfg.genesis_timestamp = MassaTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    tools::consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // * create 40 normal blocks in each thread: in slot 1 they have genesis parents, in slot 2 they have slot 1 parents
            // create a valid block for slot 1
            let mut valid_hasht0 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 0),
                genesis_hashes.clone(),
                true,
                false,
                staking_keys[0],
            )
            .await;

            // create a valid block on the other thread.
            let mut valid_hasht1 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 1),
                genesis_hashes.clone(),
                true,
                false,
                staking_keys[0],
            )
            .await;

            // and loop for the 39 other blocks
            for i in 0..39 {
                valid_hasht0 = tools::create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(i + 2, 0),
                    vec![valid_hasht0, valid_hasht1],
                    true,
                    false,
                    staking_keys[0],
                )
                .await;

                // create a valid block on the other thread.
                valid_hasht1 = tools::create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(i + 2, 1),
                    vec![valid_hasht0, valid_hasht1],
                    true,
                    false,
                    staking_keys[0],
                )
                .await;
            }

            // create 1 block in thread 0 slot 41 with missed block as parent
            valid_hasht0 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(41, 0),
                vec![valid_hasht0, valid_hasht1],
                true,
                false,
                staking_keys[0],
            )
            .await;

            if let Some(block) = consensus_command_sender
                .get_active_block(valid_hasht0)
                .await
                .unwrap()
            {
                tools::propagate_block(&mut protocol_controller, block, false, 1000).await;
            };
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
async fn test_double_staking() {
    // setup logging
    // stderrlog::new()
    //     .verbosity(3)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();

    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 32;

    // to avoid timing pb for block in the future
    cfg.genesis_timestamp = MassaTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    tools::consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // * create 40 normal blocks in each thread: in slot 1 they have genesis parents, in slot 2 they have slot 1 parents
            // create a valid block for slot 1
            let mut valid_hasht0 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 0),
                genesis_hashes.clone(),
                true,
                false,
                staking_keys[0],
            )
            .await;

            // create a valid block on the other thread.
            let mut valid_hasht1 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 1),
                genesis_hashes.clone(),
                true,
                false,
                staking_keys[0],
            )
            .await;

            // and loop for the 39 other blocks
            for i in 0..39 {
                valid_hasht0 = tools::create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(i + 2, 0),
                    vec![valid_hasht0, valid_hasht1],
                    true,
                    false,
                    staking_keys[0],
                )
                .await;

                // create a valid block on the other thread.
                valid_hasht1 = tools::create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(i + 2, 1),
                    vec![valid_hasht0, valid_hasht1],
                    true,
                    false,
                    staking_keys[0],
                )
                .await;
            }

            // same creator same slot, different block
            let operation_merkle_root = Hash::compute_from("42".as_bytes());
            let (hash_1, block_1, _key) = tools::create_block_with_merkle_root(
                &cfg,
                operation_merkle_root,
                Slot::new(41, 0),
                vec![valid_hasht0, valid_hasht1],
                staking_keys[0],
            );
            tools::propagate_block(&mut protocol_controller, block_1, true, 150).await;

            let operation_merkle_root =
                Hash::compute_from("so long and thanks for all the fish".as_bytes());
            let (hash_2, block_2, _key) = tools::create_block_with_merkle_root(
                &cfg,
                operation_merkle_root,
                Slot::new(41, 0),
                vec![valid_hasht0, valid_hasht1],
                staking_keys[0],
            );
            tools::propagate_block(&mut protocol_controller, block_2, true, 150).await;

            let graph = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .unwrap();
            let cliques_1 = tools::get_cliques(&graph, hash_1);
            let cliques_2 = tools::get_cliques(&graph, hash_2);
            assert!(cliques_1.is_disjoint(&cliques_2));
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
async fn test_test_parents() {
    // // setup logging
    // stderrlog::new()
    //     .verbosity(4)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();

    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);

    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 32;

    // to avoid timing pb for block in the future
    cfg.genesis_timestamp = MassaTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    tools::consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // * create 2 normal blocks in each thread: in slot 1 they have genesis parents, in slot 2 they have slot 1 parents
            // create a valid block for slot 1
            let valid_hasht0s1 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 0),
                genesis_hashes.clone(),
                true,
                false,
                staking_keys[0],
            )
            .await;

            // create a valid block on the other thread.
            let valid_hasht1s1 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 1),
                genesis_hashes.clone(),
                true,
                false,
                staking_keys[0],
            )
            .await;

            // create a valid block for slot 2
            let valid_hasht0s2 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(2, 0),
                vec![valid_hasht0s1, valid_hasht1s1],
                true,
                false,
                staking_keys[0],
            )
            .await;

            // create a valid block on the other thread.
            tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(2, 1),
                vec![valid_hasht0s1, valid_hasht1s1],
                true,
                false,
                staking_keys[0],
            )
            .await;

            // * create 1 block in t0s3 with parents (t0s2, t1s0)
            // create a valid block for slot 2
            tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(3, 0),
                vec![valid_hasht0s2, genesis_hashes[1usize]],
                false,
                false,
                staking_keys[0],
            )
            .await;

            // * create 1 block in t1s3 with parents (t0s0, t0s0)
            tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(3, 1),
                vec![genesis_hashes[0usize], genesis_hashes[0usize]],
                false,
                false,
                staking_keys[0],
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
