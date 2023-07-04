use massa_consensus_exports::ConsensusConfig;
use massa_models::{address::Address, block::BlockGraphStatus, slot::Slot};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;

use super::tools::{
    consensus_without_pool_test, register_block_and_process_with_tc, TestController,
};

// Always use latest blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_fts_latest_blocks_as_parents() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(500),
        thread_count: 4,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 8,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());

    consensus_without_pool_test(
        cfg.clone(),
        move |protocol_controller,
              consensus_controller,
              consensus_event_receiver,
              selector_controller,
              selector_receiver| {
            let genesis = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status")
                .genesis_blocks;

            let tc = TestController {
                creator: staking_key,
                consensus_controller,
                selector_receiver,
                storage,
                staking_address,
                timeout_ms: 1000,
            };

            // Period 1.
            let block_1_0 = register_block_and_process_with_tc(
                Slot::new(1, 0),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1], genesis[2], genesis[3]],
                &tc,
            );
            let block_1_2 = register_block_and_process_with_tc(
                Slot::new(1, 2),
                vec![block_1_0.id, block_1_1.id, genesis[2], genesis[3]],
                &tc,
            );
            let block_1_3 = register_block_and_process_with_tc(
                Slot::new(1, 3),
                vec![block_1_0.id, block_1_1.id, block_1_2.id, genesis[3]],
                &tc,
            );

            // Period 2.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id, block_1_2.id, block_1_3.id],
                &tc,
            );
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_2_0.id, block_1_1.id, block_1_2.id, block_1_3.id],
                &tc,
            );
            let block_2_2 = register_block_and_process_with_tc(
                Slot::new(2, 2),
                vec![block_2_0.id, block_2_1.id, block_1_2.id, block_1_3.id],
                &tc,
            );
            let block_2_3 = register_block_and_process_with_tc(
                Slot::new(2, 3),
                vec![block_2_0.id, block_2_1.id, block_2_2.id, block_1_3.id],
                &tc,
            );

            // Period 3, thread 0.
            let block_3_0 = register_block_and_process_with_tc(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id, block_2_2.id, block_2_3.id],
                &tc,
            );

            // block_1_0 has not been finalized yet.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_1_2.id,
                    block_1_3.id
                ]),
                [
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 3, thread 1.
            let block_3_1 = register_block_and_process_with_tc(
                Slot::new(3, 1),
                vec![block_3_0.id, block_2_1.id, block_2_2.id, block_2_3.id],
                &tc,
            );

            // block_1_0 has been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_1_2.id,
                    block_1_3.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 3, thread 2.
            let block_3_2 = register_block_and_process_with_tc(
                Slot::new(3, 2),
                vec![block_3_0.id, block_3_1.id, block_2_2.id, block_2_3.id],
                &tc,
            );

            // block_1_1 has been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_1_2.id,
                    block_1_3.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 3, thread 3.
            let _block_3_3 = register_block_and_process_with_tc(
                Slot::new(3, 3),
                vec![block_3_0.id, block_3_1.id, block_3_2.id, block_2_3.id],
                &tc,
            );

            // block_1_2 has been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_1_2.id,
                    block_1_3.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            (
                protocol_controller,
                tc.consensus_controller,
                consensus_event_receiver,
                selector_controller,
                tc.selector_receiver,
            )
        },
    );
}

// Check max cliques when there are multiple incompatibilities.
// As one of the max cliques is extended, others are discarded.
#[test]
fn test_fts_multiple_max_cliques_1() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(500),
        thread_count: 4,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 8,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());

    consensus_without_pool_test(
        cfg.clone(),
        move |protocol_controller,
              consensus_controller,
              consensus_event_receiver,
              selector_controller,
              selector_receiver| {
            let genesis = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status")
                .genesis_blocks;

            let tc = TestController {
                creator: staking_key,
                consensus_controller,
                selector_receiver,
                storage,
                staking_address,
                timeout_ms: 1000,
            };

            // Period 1.
            let block_1_0 = register_block_and_process_with_tc(
                Slot::new(1, 0),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &tc,
            );
            let block_1_2 = register_block_and_process_with_tc(
                Slot::new(1, 2),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &tc,
            );
            let block_1_3 = register_block_and_process_with_tc(
                Slot::new(1, 3),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &tc,
            );

            // Period 2.
            // block_2_0 and block_2_2 are grandpa incompatibilities.
            // block_2_1 and block_2_3 are grandpa incompatibilities.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id, genesis[2], block_1_3.id],
                &tc,
            );
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id, block_1_2.id, genesis[3]],
                &tc,
            );
            let block_2_2 = register_block_and_process_with_tc(
                Slot::new(2, 2),
                vec![genesis[0], block_1_1.id, block_1_2.id, block_1_3.id],
                &tc,
            );
            let block_2_3 = register_block_and_process_with_tc(
                Slot::new(2, 3),
                vec![block_1_0.id, genesis[1], block_1_2.id, block_1_3.id],
                &tc,
            );

            // Should have 4 max cliques:
            // [block_2_0, block_2_1, (blocks in other periods...)]
            // [block_2_0, block_2_3, (blocks in other periods...)]
            // [block_2_1, block_2_2, (blocks in other periods...)]
            // [block_2_2, block_2_3, (blocks in other periods...)]
            let mut status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                4,
                "incorrect number of max cliques"
            );
            for i in 0..4 {
                if status.max_cliques[i].block_ids.contains(&block_2_0.id) {
                    assert!(
                        !status.max_cliques[i].block_ids.contains(&block_2_2.id),
                        "incorrect max clique"
                    );
                    assert!(
                        status.max_cliques[i].block_ids.contains(&block_2_1.id)
                            || status.max_cliques[i].block_ids.contains(&block_2_3.id),
                        "incorrect max clique"
                    );
                } else if status.max_cliques[i].block_ids.contains(&block_2_1.id) {
                    assert!(
                        !status.max_cliques[i].block_ids.contains(&block_2_3.id),
                        "incorrect max clique"
                    );
                    assert!(
                        status.max_cliques[i].block_ids.contains(&block_2_0.id)
                            || status.max_cliques[i].block_ids.contains(&block_2_2.id),
                        "incorrect max clique"
                    );
                } else if status.max_cliques[i].block_ids.contains(&block_2_2.id) {
                    assert!(
                        !status.max_cliques[i].block_ids.contains(&block_2_0.id),
                        "incorrect max clique"
                    );
                    assert!(
                        status.max_cliques[i].block_ids.contains(&block_2_1.id)
                            || status.max_cliques[i].block_ids.contains(&block_2_3.id),
                        "incorrect max clique"
                    );
                } else if status.max_cliques[i].block_ids.contains(&block_2_3.id) {
                    assert!(
                        !status.max_cliques[i].block_ids.contains(&block_2_1.id),
                        "incorrect max clique"
                    );
                    assert!(
                        status.max_cliques[i].block_ids.contains(&block_2_0.id)
                            || status.max_cliques[i].block_ids.contains(&block_2_2.id),
                        "incorrect max clique"
                    );
                }
            }

            // Determine the index of the max clique which will be extended.
            // Check max cliques' fitnesses.
            for i in 0..4 {
                assert_eq!(
                    status.max_cliques[i].block_ids.len(),
                    6,
                    "incorrect max clique's fitness"
                );
            }

            // Check gi_head.
            if let Some(h) = status.gi_head.get(&block_2_0.id) {
                assert!(h.contains(&block_2_2.id), "incorrect gi_head");
                assert!(
                    !h.contains(&block_2_1.id) && !h.contains(&block_2_3.id),
                    "incorrect gi_head"
                );
            } else {
                panic!("missing block in gi_head");
            }
            if let Some(h) = status.gi_head.get(&block_2_1.id) {
                assert!(h.contains(&block_2_3.id), "incorrect gi_head");
                assert!(
                    !h.contains(&block_2_0.id) && !h.contains(&block_2_2.id),
                    "incorrect gi_head"
                );
            } else {
                panic!("missing block in gi_head");
            }
            if let Some(h) = status.gi_head.get(&block_2_2.id) {
                assert!(h.contains(&block_2_0.id), "incorrect gi_head");
                assert!(
                    !h.contains(&block_2_1.id) && !h.contains(&block_2_3.id),
                    "incorrect gi_head"
                );
            } else {
                panic!("missing block in gi_head");
            }
            if let Some(h) = status.gi_head.get(&block_2_3.id) {
                assert!(h.contains(&block_2_1.id), "incorrect gi_head");
                assert!(
                    !h.contains(&block_2_0.id) && !h.contains(&block_2_2.id),
                    "incorrect gi_head"
                );
            } else {
                panic!("missing block in gi_head");
            }

            // Period 3.
            // Based on the max clique [block_2_0, block_2_1, (blocks in other periods...)].
            let block_3_0 = register_block_and_process_with_tc(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id, block_1_2.id, block_1_3.id],
                &tc,
            );
            let block_3_1 = register_block_and_process_with_tc(
                Slot::new(3, 1),
                vec![block_2_0.id, block_2_1.id, block_1_2.id, block_1_3.id],
                &tc,
            );
            let block_3_2 = register_block_and_process_with_tc(
                Slot::new(3, 2),
                vec![block_2_0.id, block_2_1.id, block_1_2.id, block_1_3.id],
                &tc,
            );
            let block_3_3 = register_block_and_process_with_tc(
                Slot::new(3, 3),
                vec![block_2_0.id, block_2_1.id, block_1_2.id, block_1_3.id],
                &tc,
            );

            // Should still have 4 max cliques now.
            status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                4,
                "incorrect number of max cliques"
            );

            // Check max cliques' fitnesses.
            for i in 0..4 {
                if status.max_cliques[i].block_ids.contains(&block_2_0.id)
                    && status.max_cliques[i].block_ids.contains(&block_2_1.id)
                {
                    assert_eq!(
                        status.max_cliques[i].block_ids.len(),
                        10,
                        "incorrect max clique's fitness"
                    );
                } else {
                    assert_eq!(
                        status.max_cliques[i].block_ids.len(),
                        6,
                        "incorrect max clique's fitness"
                    );
                }
            }

            // Period 4, thread 0, 1, and 2.
            let block_4_0 = register_block_and_process_with_tc(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id, block_3_2.id, block_3_3.id],
                &tc,
            );
            let block_4_1 = register_block_and_process_with_tc(
                Slot::new(4, 1),
                vec![block_3_0.id, block_3_1.id, block_3_2.id, block_3_3.id],
                &tc,
            );
            let block_4_2 = register_block_and_process_with_tc(
                Slot::new(4, 2),
                vec![block_3_0.id, block_3_1.id, block_3_2.id, block_3_3.id],
                &tc,
            );

            // block_1_0 and block_1_1 have been finalized while block_1_2 and block_1_3 have not.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_1_2.id,
                    block_1_3.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 4, thread 3.
            let block_4_3 = register_block_and_process_with_tc(
                Slot::new(4, 3),
                vec![block_3_0.id, block_3_1.id, block_3_2.id, block_3_3.id],
                &tc,
            );

            // block_1_2 and block_1_3 have been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_1_2.id,
                    block_1_3.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                ],
                "incorrect block statuses"
            );

            // Should still have 4 max cliques now.
            status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                4,
                "incorrect number of max cliques"
            );

            // Period 5.
            let _block_5_0 = register_block_and_process_with_tc(
                Slot::new(5, 0),
                vec![block_4_0.id, block_4_1.id, block_4_2.id, block_4_3.id],
                &tc,
            );

            // Should have only one max clique now.
            status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                1,
                "incorrect number of max cliques"
            );

            // block_2_0 and block_2_1 have been finalized while block_2_2 and block_2_3 have been discarded.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
                    block_2_0.id,
                    block_2_1.id,
                    block_2_2.id,
                    block_2_3.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Discarded,
                    BlockGraphStatus::Discarded,
                ],
                "incorrect block statuses"
            );

            (
                protocol_controller,
                tc.consensus_controller,
                consensus_event_receiver,
                selector_controller,
                tc.selector_receiver,
            )
        },
    );
}

// Check max cliques when there are multiple incompatibilities.
// Two of the max cliques are extended.
#[test]
fn test_fts_multiple_max_cliques_2() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(500),
        thread_count: 4,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 8,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());

    consensus_without_pool_test(
        cfg.clone(),
        move |protocol_controller,
              consensus_controller,
              consensus_event_receiver,
              selector_controller,
              selector_receiver| {
            let genesis = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status")
                .genesis_blocks;

            let tc = TestController {
                creator: staking_key,
                consensus_controller,
                selector_receiver,
                storage,
                staking_address,
                timeout_ms: 1000,
            };

            // Period 1.
            let block_1_0 = register_block_and_process_with_tc(
                Slot::new(1, 0),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &tc,
            );
            let block_1_2 = register_block_and_process_with_tc(
                Slot::new(1, 2),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &tc,
            );
            let block_1_3 = register_block_and_process_with_tc(
                Slot::new(1, 3),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &tc,
            );

            // Period 2.
            // block_2_0 and block_2_2 are grandpa incompatibilities.
            // block_2_1 and block_2_3 are grandpa incompatibilities.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id, genesis[2], block_1_3.id],
                &tc,
            );
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id, block_1_2.id, genesis[3]],
                &tc,
            );
            let block_2_2 = register_block_and_process_with_tc(
                Slot::new(2, 2),
                vec![genesis[0], block_1_1.id, block_1_2.id, block_1_3.id],
                &tc,
            );
            let block_2_3 = register_block_and_process_with_tc(
                Slot::new(2, 3),
                vec![block_1_0.id, genesis[1], block_1_2.id, block_1_3.id],
                &tc,
            );

            // Should have 4 max cliques:
            // [block_2_0, block_2_1, (blocks in other periods...)]
            // [block_2_0, block_2_3, (blocks in other periods...)]
            // [block_2_1, block_2_2, (blocks in other periods...)]
            // [block_2_2, block_2_3, (blocks in other periods...)]
            let mut status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                4,
                "incorrect number of max cliques"
            );

            // Ignore other checks performed in test_fts_multiple_max_cliques_1.

            // Period 3 to 6.
            let mut prev_blocks = vec![block_2_0.id, block_2_1.id, block_2_2.id, block_2_3.id];
            for i in 3..7 {
                // Max clique 1.
                let new_block_0 = register_block_and_process_with_tc(
                    Slot::new(i, 0),
                    vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                    &tc,
                );
                let new_block_1 = register_block_and_process_with_tc(
                    Slot::new(i, 1),
                    vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                    &tc,
                );

                // Max clique 2.
                let new_block_2 = register_block_and_process_with_tc(
                    Slot::new(i, 2),
                    vec![block_1_0.id, block_1_1.id, prev_blocks[2], prev_blocks[3]],
                    &tc,
                );
                let new_block_3 = register_block_and_process_with_tc(
                    Slot::new(i, 3),
                    vec![block_1_0.id, block_1_1.id, prev_blocks[2], prev_blocks[3]],
                    &tc,
                );

                prev_blocks = vec![
                    new_block_0.id,
                    new_block_1.id,
                    new_block_2.id,
                    new_block_3.id,
                ];
            }

            // Blocks in period 1 have all be finalized, while none in period 2 have been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_1_2.id,
                    block_1_3.id,
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                ],
                "incorrect block statuses"
            );
            assert_ne!(
                tc.consensus_controller.get_block_statuses(&[
                    block_2_0.id,
                    block_2_1.id,
                    block_2_2.id,
                    block_2_3.id,
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                ],
                "incorrect block statuses"
            );
            println!(
                "> block_2_* status: {:?}",
                tc.consensus_controller.get_block_statuses(&[
                    block_2_0.id,
                    block_2_1.id,
                    block_2_2.id,
                    block_2_3.id
                ])
            );

            // Should still have 4 max cliques now.
            status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                4,
                "incorrect number of max cliques"
            );

            // Check max cliques' fitnesses.
            for i in 0..4 {
                if status.max_cliques[i].block_ids.contains(&block_2_0.id)
                    && status.max_cliques[i].block_ids.contains(&block_2_1.id)
                    || status.max_cliques[i].block_ids.contains(&block_2_2.id)
                        && status.max_cliques[i].block_ids.contains(&block_2_3.id)
                {
                    assert_eq!(
                        status.max_cliques[i].block_ids.len(),
                        10,
                        "incorrect max clique's fitness"
                    );
                } else {
                    assert_eq!(
                        status.max_cliques[i].block_ids.len(),
                        2,
                        "incorrect max clique's fitness"
                    );
                }
            }

            // Period 7, max clique 1.
            let new_block_0 = register_block_and_process_with_tc(
                Slot::new(7, 0),
                vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                &tc,
            );
            let new_block_1 = register_block_and_process_with_tc(
                Slot::new(7, 1),
                vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                &tc,
            );

            // Period 7, max clique 2.
            let new_block_2 = register_block_and_process_with_tc(
                Slot::new(7, 2),
                vec![block_1_0.id, block_1_1.id, prev_blocks[2], prev_blocks[3]],
                &tc,
            );
            let new_block_3 = register_block_and_process_with_tc(
                Slot::new(7, 3),
                vec![block_1_0.id, block_1_1.id, prev_blocks[2], prev_blocks[3]],
                &tc,
            );

            // Should still have 2 max cliques now.
            status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                2,
                "incorrect number of max cliques"
            );

            // Period 8 to 15.
            prev_blocks = vec![
                new_block_0.id,
                new_block_1.id,
                new_block_2.id,
                new_block_3.id,
            ];
            for i in 8..16 {
                // Max clique 1.
                let new_block_0 = register_block_and_process_with_tc(
                    Slot::new(i, 0),
                    vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                    &tc,
                );
                let new_block_1 = register_block_and_process_with_tc(
                    Slot::new(i, 1),
                    vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                    &tc,
                );

                // Max clique 2.
                let new_block_2 = register_block_and_process_with_tc(
                    Slot::new(i, 2),
                    vec![block_1_0.id, block_1_1.id, prev_blocks[2], prev_blocks[3]],
                    &tc,
                );
                let new_block_3 = register_block_and_process_with_tc(
                    Slot::new(i, 3),
                    vec![block_1_0.id, block_1_1.id, prev_blocks[2], prev_blocks[3]],
                    &tc,
                );

                prev_blocks = vec![
                    new_block_0.id,
                    new_block_1.id,
                    new_block_2.id,
                    new_block_3.id,
                ];
            }

            // Should still have 2 max cliques now.
            status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                2,
                "incorrect number of max cliques"
            );

            (
                protocol_controller,
                tc.consensus_controller,
                consensus_event_receiver,
                selector_controller,
                tc.selector_receiver,
            )
        },
    );
}
