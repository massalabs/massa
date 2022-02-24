// Copyright (c) 2022 MASSA LABS <info@massa.net>

// RUST_BACKTRACE=1 cargo test scenarios106 -- --nocapture

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;

use massa_models::{init_serialization_context, Slot};
use massa_signature::{generate_random_private_key, PrivateKey};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_consensus_asks_for_block() {
    init_serialization_context(massa_models::SerializationContext::default());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        t0: 500.into(),
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

            validate_ask_for_block(&mut protocol_controller, hasht0s1, 1000).await;
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
async fn test_consensus_does_not_ask_for_block() {
    init_serialization_context(massa_models::SerializationContext::default());
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
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
            let (hasht0s1, t0s1, _) = create_block(
                &cfg,
                Slot::new(1 + start_slot, 0),
                genesis_hashes.clone(),
                staking_keys[0],
            );
            let header = t0s1.header.clone();

            // Send the actual block.
            protocol_controller.receive_block(t0s1).await;

            // block t0s1 is propagated
            let hash_list = vec![hasht0s1];
            validate_propagate_block_in_list(
                &mut protocol_controller,
                &hash_list,
                3000 + start_slot as u64 * 1000,
            )
            .await;

            // Send the hash
            protocol_controller.receive_header(header).await;

            // Consensus should not ask for the block, so the time-out should be hit.
            validate_does_not_ask_for_block(&mut protocol_controller, &hasht0s1, 10).await;
            (
                protocol_controller,
                consensus_command_sender,
                consensus_event_receiver,
            )
        },
    )
    .await;
}
