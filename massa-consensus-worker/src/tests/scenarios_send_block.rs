// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test scenarios106 -- --nocapture

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;

use massa_models::Slot;
use massa_signature::KeyPair;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_consensus_sends_block_to_peer_who_asked_for_it() {
    let staking_keys: Vec<KeyPair> = (0..1).map(|_| KeyPair::generate()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        future_block_processing_max_periods: 50,
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let start_slot = 3;
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // create test blocks
            let slot = Slot::new(1 + start_slot, 0);
            let draw = consensus_command_sender
                .get_selection_draws(slot, Slot::new(2 + start_slot, 0))
                .await
                .expect("could not get selection draws.")[0]
                .1
                 .0;
            let creator = get_creator_for_draw(&draw, &staking_keys.clone());
            let t0s1 = create_block(
                &cfg,
                Slot::new(1 + start_slot, 0),
                genesis_hashes.clone(),
                &creator,
            );

            // Send the actual block.
            protocol_controller.receive_block(t0s1.clone()).await;

            // block t0s1 is propagated
            let hash_list = vec![t0s1.id];
            validate_propagate_block_in_list(
                &mut protocol_controller,
                &hash_list,
                3000 + start_slot as u64 * 1000,
            )
            .await;

            // Ask for the block to consensus.
            protocol_controller
                .receive_get_active_blocks(vec![t0s1.id])
                .await;

            // Consensus should respond with results including the block.
            validate_block_found(&mut protocol_controller, &t0s1.id, 100).await;
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
async fn test_consensus_block_not_found() {
    let staking_keys: Vec<KeyPair> = (0..1).map(|_| KeyPair::generate()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        future_block_processing_max_periods: 50,
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let start_slot = 3;
            let genesis_hashes = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            // create test blocks
            let t0s1 = create_block(
                &cfg,
                Slot::new(1 + start_slot, 0),
                genesis_hashes.clone(),
                &staking_keys[0],
            );

            // Ask for the block to consensus.
            protocol_controller
                .receive_get_active_blocks(vec![t0s1.id])
                .await;

            // Consensus should not have the block.
            validate_block_not_found(&mut protocol_controller, &t0s1.id, 100).await;
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
