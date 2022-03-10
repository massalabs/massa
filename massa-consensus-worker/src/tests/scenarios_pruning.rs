// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;

use massa_models::{BlockId, Slot};
use massa_signature::{generate_random_private_key, PrivateKey};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_pruning_of_discarded_blocks() {
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        future_block_processing_max_periods: 50,
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            // Send more bad blocks than the max number of cached discarded.
            for i in 0..(cfg.max_discarded_blocks + 5) as u64 {
                // Too far into the future.
                let _ = create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(100000000 + i, 0),
                    parents.clone(),
                    false,
                    false,
                    staking_keys[0],
                )
                .await;
            }

            let status = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status");
            assert!(status.discarded_blocks.len() <= cfg.max_discarded_blocks);

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
async fn test_pruning_of_awaiting_slot_blocks() {
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        t0: 1000.into(),
        future_block_processing_max_periods: 50,
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            // Send more blocks in the future than the max number of future processing blocks.
            for i in 0..(cfg.max_future_processing_blocks + 5) as u64 {
                // Too far into the future.
                let _ = create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(10 + i, 0),
                    parents.clone(),
                    false,
                    false,
                    staking_keys[0],
                )
                .await;
            }

            let status = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status");
            assert!(status.discarded_blocks.len() <= cfg.max_future_processing_blocks);
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
async fn test_pruning_of_awaiting_dependencies_blocks_with_discarded_dependency() {
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        t0: 200.into(),
        future_block_processing_max_periods: 50,
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let parents: Vec<BlockId> = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .best_parents
                .iter()
                .map(|(b, _p)| *b)
                .collect();

            // Too far into the future.
            let (bad_parent, bad_block, _) =
                create_block(&cfg, Slot::new(10000, 0), parents.clone(), staking_keys[0]);

            for i in 1..4 {
                // Sent several headers with the bad parent as dependency.
                let _ = create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(i, 0),
                    vec![bad_parent, parents.clone()[0]],
                    false,
                    false,
                    staking_keys[0],
                )
                .await;
            }

            // Now, send the bad parent.
            protocol_controller.receive_header(bad_block.header).await;
            validate_notpropagate_block_in_list(&mut protocol_controller, &vec![bad_parent], 10)
                .await;

            // Eventually, all blocks will be discarded due to their bad parent.
            // Note the parent too much in the future will not be discarded, but ignored.
            loop {
                let status = consensus_command_sender
                    .get_block_graph_status(None, None)
                    .await
                    .expect("could not get block graph status");
                if status.discarded_blocks.len() == 3 {
                    break;
                }
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
