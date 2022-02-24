// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test scenarios106 -- --nocapture

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;

use massa_models::Slot;
use massa_signature::{generate_random_private_key, PrivateKey};
use serial_test::serial;
use std::collections::HashSet;
use std::iter::FromIterator;

#[tokio::test]
#[serial]
async fn test_wishlist_delta_with_empty_remove() {
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        future_block_processing_max_periods: 50,
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
            let slot = Slot::new(1, 0);
            let draw = consensus_command_sender
                .get_selection_draws(slot, Slot::new(2, 0))
                .await
                .expect("could not get selection draws.")[0]
                .1
                 .0;
            let creator = get_creator_for_draw(&draw, &staking_keys.clone());
            let (hasht0s1, t0s1, _) =
                create_block(&cfg, Slot::new(1, 0), genesis_hashes.clone(), creator);

            // send header for block t0s1
            protocol_controller
                .receive_header(t0s1.header.clone())
                .await;

            let expected_new = HashSet::from_iter(vec![hasht0s1].into_iter());
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
            )
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn test_wishlist_delta_remove() {
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        future_block_processing_max_periods: 50,
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
            // send header for block t0s1
            protocol_controller
                .receive_header(t0s1.header.clone())
                .await;

            let expected_new = HashSet::from_iter(vec![hasht0s1].into_iter());
            let expected_remove = HashSet::from_iter(vec![].into_iter());
            validate_wishlist(
                &mut protocol_controller,
                expected_new,
                expected_remove,
                cfg.t0.saturating_add(1000.into()).to_millis(), // leave 1sec extra for init and margin,
            )
            .await;

            protocol_controller.receive_block(t0s1.clone()).await;
            let expected_new = HashSet::from_iter(vec![].into_iter());
            let expected_remove = HashSet::from_iter(vec![hasht0s1].into_iter());
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
            )
        },
    )
    .await;
}
