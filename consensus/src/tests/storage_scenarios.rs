// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::tests::tools::{self, generate_ledger_file};
use models::Slot;
use serial_test::serial;
use std::collections::HashMap;
use time::UTime;
use tokio::time::sleep;

//create 2 clique. Extend the first until the second is discarded.
//verify the discarded block are in storage.
//verify that genesis and other click blocks aren't in storage.
#[tokio::test]
#[serial]
async fn test_storage() {
    // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/
    let ledger_file = generate_ledger_file(&HashMap::new());
    let staking_keys: Vec<crypto::signature::PrivateKey> = (0..1)
        .map(|_| crypto::generate_random_private_key())
        .collect();
    let staking_file = tools::generate_staking_keys_file(&staking_keys);
    let roll_counts_file = tools::generate_default_roll_counts_file(staking_keys.clone());
    let mut cfg = tools::default_consensus_config(
        ledger_file.path(),
        roll_counts_file.path(),
        staking_file.path(),
    );
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 10;
    cfg.max_discarded_blocks = 1;

    // to avoid timing problems for blocks in the future
    cfg.genesis_timestamp = UTime::now(0)
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    // start storage
    let storage_access = tools::start_storage();

    tools::consensus_without_pool_test(
        cfg.clone(),
        Some(storage_access.clone()),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status()
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            //create a valids block for thread 0
            let valid_hasht0s1 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 0),
                genesis_hashes.clone(),
                true,
                false,
                staking_keys[0].clone(),
            )
            .await;
            //create a valid block on the other thread.
            let valid_hasht1s1 = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 1),
                genesis_hashes.clone(),
                true,
                false,
                staking_keys[0].clone(),
            )
            .await;

            //Create other clique bock T0S2
            let fork_block_hash = tools::create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(2, 0),
                genesis_hashes.clone(),
                true,
                false,
                staking_keys[0].clone(),
            )
            .await;

            assert!(!&storage_access.contains(fork_block_hash).await.unwrap());

            //extend first clique
            let mut parentt0sn_hash = valid_hasht0s1;
            let mut parentt1sn_hash = valid_hasht1s1;
            for period in 3..=12 {
                let block_hash_0 = tools::create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(period, 0),
                    vec![parentt0sn_hash, parentt1sn_hash],
                    true,
                    false,
                    staking_keys[0].clone(),
                )
                .await;
                parentt0sn_hash = block_hash_0;

                let block_hash = tools::create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(period, 1),
                    vec![parentt0sn_hash, parentt1sn_hash],
                    true,
                    false,
                    staking_keys[0].clone(),
                )
                .await;
                parentt1sn_hash = block_hash;
            }

            // wait until consensus is pruned
            sleep(
                cfg.block_db_prune_timer
                    .saturating_add(150.into())
                    .to_duration(),
            )
            .await;

            assert!(!&storage_access.contains(fork_block_hash).await.unwrap());
            assert!(&storage_access.contains(genesis_hashes[0]).await.unwrap());
            assert!(&storage_access.contains(genesis_hashes[1]).await.unwrap());
            assert!(&storage_access.contains(valid_hasht0s1).await.unwrap());
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
