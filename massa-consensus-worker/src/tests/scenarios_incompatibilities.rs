// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::tools::*;
use massa_consensus_exports::ConsensusConfig;

use massa_models::{BlockId, Slot};
use massa_signature::{generate_random_private_key, PrivateKey};
use serial_test::serial;
use std::collections::{HashSet, VecDeque};

#[tokio::test]
#[serial]
async fn test_thread_incompatibility() {
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

            let hash_1 = create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 0),
                parents.clone(),
                true,
                false,
                staking_keys[0],
            )
            .await;

            let hash_2 = create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 1),
                parents.clone(),
                true,
                false,
                staking_keys[0],
            )
            .await;

            let hash_3 = create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(2, 0),
                parents.clone(),
                true,
                false,
                staking_keys[0],
            )
            .await;

            let status = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status");

            if hash_1 > hash_3 {
                assert_eq!(status.best_parents[0].0, hash_3);
            } else {
                assert_eq!(status.best_parents[0].0, hash_1);
            }
            assert_eq!(status.best_parents[1].0, hash_2);

            assert!(if let Some(h) = status.gi_head.get(&hash_3) {
                h.contains(&hash_1)
            } else {
                panic!("missing hash in gi_head")
            });

            assert_eq!(status.max_cliques.len(), 2);

            for clique in status.max_cliques.clone() {
                if clique.block_ids.contains(&hash_1) && clique.block_ids.contains(&hash_3) {
                    panic!("incompatible blocks in the same clique")
                }
            }

            let mut current_period = 3;
            let mut parents = vec![hash_1, hash_2];
            for _ in 0..3 {
                let hash = create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(current_period, 0),
                    parents.clone(),
                    true,
                    false,
                    staking_keys[0],
                )
                .await;
                current_period += 1;
                parents[0] = hash;
            }

            let status = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status");

            assert!(if let Some(h) = status.gi_head.get(&hash_3) {
                h.contains(&status.best_parents[0].0)
            } else {
                panic!("missing block in clique")
            });

            let mut parents = vec![status.best_parents[0].0, hash_2];
            let mut current_period = 8;
            for _ in 0..30 {
                let (hash, b, _) = create_block(
                    &cfg,
                    Slot::new(current_period, 0),
                    parents.clone(),
                    staking_keys[0],
                );
                current_period += 1;
                parents[0] = hash;
                protocol_controller.receive_block(b).await;

                // Note: higher timeout required.
                validate_propagate_block_in_list(&mut protocol_controller, &vec![hash], 5000).await;
            }

            let status = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status");

            assert_eq!(status.max_cliques.len(), 1);

            // clique should have been deleted by now
            let parents = vec![hash_3, hash_2];
            let _ = create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(40, 0),
                parents.clone(),
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

#[tokio::test]
#[serial]
async fn test_grandpa_incompatibility() {
    let staking_keys: Vec<PrivateKey> = (0..1).map(|_| generate_random_private_key()).collect();
    let cfg = ConsensusConfig {
        t0: 200.into(),
        future_block_processing_max_periods: 50,
        ..ConsensusConfig::default_with_staking_keys(&staking_keys)
    };

    consensus_without_pool_test(
        cfg.clone(),
        async move |mut protocol_controller, consensus_command_sender, consensus_event_receiver| {
            let genesis = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status")
                .genesis_blocks;

            let hash_1 = create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 0),
                vec![genesis[0], genesis[1]],
                true,
                false,
                staking_keys[0],
            )
            .await;

            let hash_2 = create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(1, 1),
                vec![genesis[0], genesis[1]],
                true,
                false,
                staking_keys[0],
            )
            .await;

            let hash_3 = create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(2, 0),
                vec![hash_1, genesis[1]],
                true,
                false,
                staking_keys[0],
            )
            .await;

            let hash_4 = create_and_test_block(
                &mut protocol_controller,
                &cfg,
                Slot::new(2, 1),
                vec![genesis[0], hash_2],
                true,
                false,
                staking_keys[0],
            )
            .await;

            let status = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status");

            assert!(if let Some(h) = status.gi_head.get(&hash_4) {
                h.contains(&hash_3)
            } else {
                panic!("missing block in gi_head")
            });

            assert_eq!(status.max_cliques.len(), 2);

            for clique in status.max_cliques.clone() {
                if clique.block_ids.contains(&hash_3) && clique.block_ids.contains(&hash_4) {
                    panic!("incompatible blocks in the same clique")
                }
            }

            let parents: Vec<BlockId> = status.best_parents.iter().map(|(b, _p)| *b).collect();
            if hash_4 > hash_3 {
                assert_eq!(parents[0], hash_3)
            } else {
                assert_eq!(parents[1], hash_4)
            }

            let mut latest_extra_blocks = VecDeque::new();
            for extend_i in 0..33 {
                let status = consensus_command_sender
                    .get_block_graph_status(None, None)
                    .await
                    .expect("could not get block graph status");
                let hash = create_and_test_block(
                    &mut protocol_controller,
                    &cfg,
                    Slot::new(3 + extend_i, 0),
                    status.best_parents.iter().map(|(b, _p)| *b).collect(),
                    true,
                    false,
                    staking_keys[0],
                )
                .await;

                latest_extra_blocks.push_back(hash);
                while latest_extra_blocks.len() > cfg.delta_f0 as usize + 1 {
                    latest_extra_blocks.pop_front();
                }
            }

            let latest_extra_blocks: HashSet<BlockId> = latest_extra_blocks.into_iter().collect();
            let status = consensus_command_sender
                .get_block_graph_status(None, None)
                .await
                .expect("could not get block graph status");
            assert_eq!(status.max_cliques.len(), 1, "wrong cliques (len)");
            assert_eq!(
                status.max_cliques[0]
                    .block_ids
                    .iter()
                    .cloned()
                    .collect::<HashSet<BlockId>>(),
                latest_extra_blocks,
                "wrong cliques"
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
