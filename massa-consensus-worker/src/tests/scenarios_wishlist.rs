// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test scenarios106 -- --nocapture

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;

use massa_models::slot::Slot;
use massa_signature::KeyPair;
use massa_storage::Storage;
use serial_test::serial;
use std::collections::HashSet;
use std::iter::FromIterator;

#[tokio::test]
#[serial]
async fn test_wishlist_delta_with_empty_remove() {
    let staking_keys: Vec<KeyPair> = (0..1).map(|_| KeyPair::generate()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        future_block_processing_max_periods: 50,
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver,
                    selector_controller| {
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;
            // create test blocks
            let slot = Slot::new(1, 0);
            let draw = selector_controller
                .get_selection(slot)
                .expect("could not get selection draws.")
                .producer;
            let creator = get_creator_for_draw(&draw, &staking_keys.clone());
            let t0s1 = create_block(&cfg, Slot::new(1, 0), genesis_hashes.clone(), &creator);

            // send header for block t0s1
            protocol_controller
                .receive_header(t0s1.content.header.clone())
                .await;

            let expected_new = HashSet::from_iter(vec![t0s1.id].into_iter());
            let expected_remove = HashSet::from_iter(vec![].into_iter());
            validate_wishlist(
                &mut protocol_controller,
                expected_new,
                expected_remove,
                cfg.t0.saturating_add(1000.into()).to_millis(), // leave 1sec extra for init and margin
            )
            .await;
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
                selector_controller,
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_wishlist_delta_remove() {
    let staking_keys: Vec<KeyPair> = (0..1).map(|_| KeyPair::generate()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        future_block_processing_max_periods: 50,
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    let mut storage = Storage::create_root();

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller,
                    consensus_command_sender,
                    consensus_event_receiver,
                    selector_controller| {
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // create test blocks
            let t0s1 = create_block(
                &cfg,
                Slot::new(1, 0),
                genesis_hashes.clone(),
                &staking_keys[0],
            );
            // send header for block t0s1
            protocol_controller
                .receive_header(t0s1.content.header.clone())
                .await;

            let expected_new = HashSet::from_iter(vec![t0s1.id].into_iter());
            let expected_remove = HashSet::from_iter(vec![].into_iter());
            validate_wishlist(
                &mut protocol_controller,
                expected_new,
                expected_remove,
                cfg.t0.saturating_add(1000.into()).to_millis(), // leave 1sec extra for init and margin,
            )
            .await;

            storage.store_block(t0s1.clone());
            protocol_controller
                .receive_block(t0s1.id, t0s1.content.header.content.slot, storage.clone())
                .await;
            let expected_new = HashSet::from_iter(vec![].into_iter());
            let expected_remove = HashSet::from_iter(vec![t0s1.id].into_iter());
            validate_wishlist(
                &mut protocol_controller,
                expected_new,
                expected_remove,
                1000,
            )
            .await;
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
                selector_controller,
            )
        },
    )
    .await;
}
