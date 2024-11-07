use std::{collections::HashMap, time::Duration};

use massa_consensus_exports::ConsensusConfig;
use massa_execution_exports::MockExecutionController;
use massa_models::{
    address::Address, block::BlockGraphStatus, block_id::BlockId, config::ENDORSEMENT_COUNT,
    slot::Slot,
};
use massa_pool_exports::MockPoolController;
use massa_pos_exports::{MockSelectorController, Selection};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;

use super::tools::{consensus_test, create_block, register_block};

// Always use latest blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_fts_latest_blocks_as_parents() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(500),
        thread_count: 4,
        genesis_timestamp: MassaTime::now(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 8,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller
        .expect_notify_final_cs_periods()
        .returning(|_| {});
    pool_controller
        .expect_add_denunciation_precursor()
        .returning(|_| {});
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_get_producer()
        .returning(move |_| Ok(staking_address));
    selector_controller
        .expect_get_selection()
        .returning(move |_| {
            Ok(Selection {
                producer: staking_address,
                endorsements: vec![staking_address; ENDORSEMENT_COUNT as usize],
            })
        });
    consensus_test(
        cfg.clone(),
        execution_controller,
        pool_controller,
        selector_controller,
        move |consensus_controller| {
            let genesis = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status")
                .genesis_blocks;

            // Period 1.
            let block_1_0 = create_block(
                Slot::new(1, 0),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 = create_block(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1], genesis[2], genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            let block_1_2 = create_block(
                Slot::new(1, 2),
                vec![block_1_0.id, block_1_1.id, genesis[2], genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_2.clone(), storage.clone());
            let block_1_3 = create_block(
                Slot::new(1, 3),
                vec![block_1_0.id, block_1_1.id, block_1_2.id, genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_3.clone(), storage.clone());

            // Period 2.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id, block_1_2.id, block_1_3.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![block_2_0.id, block_1_1.id, block_1_2.id, block_1_3.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            let block_2_2 = create_block(
                Slot::new(2, 2),
                vec![block_2_0.id, block_2_1.id, block_1_2.id, block_1_3.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_2.clone(), storage.clone());
            let block_2_3 = create_block(
                Slot::new(2, 3),
                vec![block_2_0.id, block_2_1.id, block_2_2.id, block_1_3.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_3.clone(), storage.clone());

            // Period 3, thread 0.
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id, block_2_2.id, block_2_3.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());

            std::thread::sleep(Duration::from_millis(1000));
            // block_1_0 has not been finalized yet.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
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
            let block_3_1 = create_block(
                Slot::new(3, 1),
                vec![block_3_0.id, block_2_1.id, block_2_2.id, block_2_3.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_1.clone(), storage.clone());

            std::thread::sleep(Duration::from_millis(1000));
            // block_1_0 has been finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
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
            let block_3_2 = create_block(
                Slot::new(3, 2),
                vec![block_3_0.id, block_3_1.id, block_2_2.id, block_2_3.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_2.clone(), storage.clone());

            std::thread::sleep(Duration::from_millis(1000));
            // block_1_1 has been finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
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
            let block_3_3 = create_block(
                Slot::new(3, 3),
                vec![block_3_0.id, block_3_1.id, block_3_2.id, block_2_3.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_3.clone(), storage.clone());

            std::thread::sleep(Duration::from_millis(1000));
            // block_1_2 has been finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
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
        thread_count: 3,
        genesis_timestamp: MassaTime::now(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 8,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller
        .expect_notify_final_cs_periods()
        .returning(|_| {});
    pool_controller
        .expect_add_denunciation_precursor()
        .returning(|_| {});
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_get_producer()
        .returning(move |_| Ok(staking_address));
    selector_controller
        .expect_get_selection()
        .returning(move |_| {
            Ok(Selection {
                producer: staking_address,
                endorsements: vec![staking_address; ENDORSEMENT_COUNT as usize],
            })
        });
    consensus_test(
        cfg.clone(),
        execution_controller,
        pool_controller,
        selector_controller,
        move |consensus_controller| {
            let genesis = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status")
                .genesis_blocks;
            std::thread::sleep(Duration::from_millis(500));

            // Period 1.
            let block_1_0 = create_block(
                Slot::new(1, 0),
                vec![genesis[0], genesis[1], genesis[2]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 = create_block(
                Slot::new(1, 1),
                vec![genesis[0], genesis[1], genesis[2]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            let block_1_2 = create_block(
                Slot::new(1, 2),
                vec![genesis[0], genesis[1], genesis[2]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_2.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(500));

            // Period 2.
            // Thread incompatibilities with every blocks of period 1
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![genesis[0], genesis[1], genesis[2]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![genesis[0], genesis[1], genesis[2]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            let block_2_2 = create_block(
                Slot::new(2, 2),
                vec![genesis[0], genesis[1], genesis[2]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_2.clone(), storage.clone());

            // Should have 4 max cliques:
            // [block_1_0, block_1_1, block_1_2]
            // [block_1_1, block_1_2, block_2_0]
            // [block_1_2, block_2_0, block_2_1]
            // [block_2_0, block_2_1, block_2_2]
            std::thread::sleep(Duration::from_millis(500));
            let mut status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");

            let hash_to_slot: HashMap<BlockId, Slot> = vec![
                (genesis[0], Slot::new(0, 0)),
                (genesis[1], Slot::new(0, 1)),
                (genesis[2], Slot::new(0, 2)),
                (block_1_0.id, Slot::new(1, 0)),
                (block_1_1.id, Slot::new(1, 1)),
                (block_1_2.id, Slot::new(1, 2)),
                (block_2_0.id, Slot::new(2, 0)),
                (block_2_1.id, Slot::new(2, 1)),
                (block_2_2.id, Slot::new(2, 2)),
            ]
            .into_iter()
            .collect();

            let mut print = vec![];
            for (from_id, to_ids) in &status.gi_head {
                print.push(format!(
                    "{:?} => {:?}",
                    hash_to_slot[from_id],
                    to_ids.iter().map(|v| hash_to_slot[v]).collect::<Vec<_>>()
                ));
            }
            print.sort();
            for i in print {
                println!("{}", i);
            }

            // print cliques
            for clique in &status.max_cliques {
                println!(
                    "clique: {:?}",
                    clique
                        .block_ids
                        .iter()
                        .map(|v| hash_to_slot[v])
                        .collect::<Vec<_>>()
                );
            }

            let sorted_cliques = |cliques: Vec<Vec<BlockId>>| -> Vec<Vec<BlockId>> {
                let mut res = cliques.clone();
                res.iter_mut().for_each(|v| v.sort());
                res.sort();
                res
            };

            let expected_cliques = sorted_cliques(vec![
                vec![block_1_0.id, block_1_1.id, block_1_2.id],
                vec![block_2_0.id, block_1_1.id, block_1_2.id],
                vec![block_2_0.id, block_2_1.id, block_1_2.id],
                vec![block_2_0.id, block_2_1.id, block_2_2.id],
            ]);

            let found_cliques: Vec<Vec<BlockId>> = sorted_cliques(
                status
                    .max_cliques
                    .iter()
                    .map(|v| v.block_ids.iter().cloned().collect())
                    .collect(),
            );

            assert_eq!(found_cliques, expected_cliques, "wrong cliques");

            // Period 3.
            // Based on the max clique [block_2_0, block_2_1, block_1_2].
            // Later, these blocks will be finalized while block_1_0, block_1_1, and block_2_2 will be discarded
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id, block_1_2.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());
            let block_3_1 = create_block(
                Slot::new(3, 1),
                vec![block_2_0.id, block_2_1.id, block_1_2.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_1.clone(), storage.clone());
            let block_3_2 = create_block(
                Slot::new(3, 2),
                vec![block_2_0.id, block_2_1.id, block_1_2.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_2.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(500));
            // Should still have 4 max cliques now.
            status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                4,
                "incorrect number of max cliques"
            );

            // Period 4, thread 0, 1, and 2.
            let block_4_0 = create_block(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id, block_3_2.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_0.clone(), storage.clone());
            let block_4_1 = create_block(
                Slot::new(4, 1),
                vec![block_3_0.id, block_3_1.id, block_3_2.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_1.clone(), storage.clone());
            let block_4_2 = create_block(
                Slot::new(4, 2),
                vec![block_3_0.id, block_3_1.id, block_3_2.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_2.clone(), storage.clone());

            std::thread::sleep(Duration::from_millis(500));
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_1_2.id,
                    block_2_0.id,
                    block_2_1.id,
                    block_2_2.id,
                ]),
                [
                    BlockGraphStatus::ActiveInAlternativeCliques,
                    BlockGraphStatus::ActiveInAlternativeCliques,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInAlternativeCliques,
                ],
                "incorrect block statuses"
            );

            // Should still have 4 max cliques now.
            status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                4,
                "incorrect number of max cliques"
            );

            // Period 5.
            let block_5_0 = create_block(
                Slot::new(5, 0),
                vec![block_4_0.id, block_4_1.id, block_4_2.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_5_0.clone(), storage.clone());
            let block_5_1 = create_block(
                Slot::new(5, 1),
                vec![block_4_0.id, block_4_1.id, block_4_2.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_5_1.clone(), storage.clone());
            let block_5_2 = create_block(
                Slot::new(5, 2),
                vec![block_4_0.id, block_4_1.id, block_4_2.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_5_2.clone(), storage.clone());

            std::thread::sleep(Duration::from_millis(500));
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_1_2.id,
                    block_2_0.id,
                    block_2_1.id,
                    block_2_2.id,
                ]),
                [
                    BlockGraphStatus::Discarded,
                    BlockGraphStatus::Discarded,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Discarded,
                ],
                "incorrect block statuses"
            );

            // Should have only one max clique now.
            status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                1,
                "incorrect number of max cliques"
            );

            let expected_cliques = sorted_cliques(vec![vec![
                block_3_0.id,
                block_3_1.id,
                block_3_2.id,
                block_4_0.id,
                block_4_1.id,
                block_4_2.id,
                block_5_0.id,
                block_5_1.id,
                block_5_2.id,
            ]]);

            let found_cliques: Vec<Vec<BlockId>> = sorted_cliques(
                status
                    .max_cliques
                    .iter()
                    .map(|v| v.block_ids.iter().cloned().collect())
                    .collect(),
            );

            assert_eq!(found_cliques, expected_cliques, "wrong cliques");
        },
    );
}

// Check max cliques when there are multiple incompatibilities.
// Three of the max cliques are extended.
#[test]
fn test_fts_multiple_max_cliques_2() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(500),
        thread_count: 4,
        genesis_timestamp: MassaTime::now(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 8,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller
        .expect_notify_final_cs_periods()
        .returning(|_| {});
    pool_controller
        .expect_add_denunciation_precursor()
        .returning(|_| {});
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_get_producer()
        .returning(move |_| Ok(staking_address));
    selector_controller
        .expect_get_selection()
        .returning(move |_| {
            Ok(Selection {
                producer: staking_address,
                endorsements: vec![staking_address; ENDORSEMENT_COUNT as usize],
            })
        });
    consensus_test(
        cfg.clone(),
        execution_controller,
        pool_controller,
        selector_controller,
        move |consensus_controller| {
            let genesis = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status")
                .genesis_blocks;

            // Period 1.
            let block_1_0 = create_block(
                Slot::new(1, 0),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 = create_block(
                Slot::new(1, 1),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            let block_1_2 = create_block(
                Slot::new(1, 2),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_2.clone(), storage.clone());
            let block_1_3 = create_block(
                Slot::new(1, 3),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_3.clone(), storage.clone());

            // Period 2.
            // Thread incompatibilities with every block of period 1
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            let block_2_2 = create_block(
                Slot::new(2, 2),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_2.clone(), storage.clone());
            let block_2_3 = create_block(
                Slot::new(2, 3),
                vec![genesis[0], genesis[1], genesis[2], genesis[3]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_3.clone(), storage.clone());

            let mut hash_to_slot: HashMap<BlockId, Slot> = vec![
                (genesis[0], Slot::new(0, 0)),
                (genesis[1], Slot::new(0, 1)),
                (genesis[2], Slot::new(0, 2)),
                (genesis[3], Slot::new(0, 3)),
                (block_1_0.id, Slot::new(1, 0)),
                (block_1_1.id, Slot::new(1, 1)),
                (block_1_2.id, Slot::new(1, 2)),
                (block_1_3.id, Slot::new(1, 3)),
                (block_2_0.id, Slot::new(2, 0)),
                (block_2_1.id, Slot::new(2, 1)),
                (block_2_2.id, Slot::new(2, 2)),
                (block_2_3.id, Slot::new(2, 3)),
            ]
            .into_iter()
            .collect();
            // Ignore other checks performed in test_fts_multiple_max_cliques_1.

            // Period 3 to 6.
            let mut prev_blocks = vec![block_2_0.id, block_2_1.id, block_2_2.id, block_2_3.id];
            for i in 3..=6 {
                // Max clique 1.
                let new_block_0 = create_block(
                    Slot::new(i, 0),
                    vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                    &staking_key,
                );
                register_block(&consensus_controller, new_block_0.clone(), storage.clone());
                let new_block_1 = create_block(
                    Slot::new(i, 1),
                    vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                    &staking_key,
                );
                register_block(&consensus_controller, new_block_1.clone(), storage.clone());
                // Max clique 2.
                let new_block_2 = create_block(
                    Slot::new(i, 2),
                    vec![block_2_0.id, block_2_1.id, prev_blocks[2], prev_blocks[3]],
                    &staking_key,
                );
                register_block(&consensus_controller, new_block_2.clone(), storage.clone());
                let new_block_3 = create_block(
                    Slot::new(i, 3),
                    vec![block_2_0.id, block_2_1.id, prev_blocks[2], prev_blocks[3]],
                    &staking_key,
                );
                register_block(&consensus_controller, new_block_3.clone(), storage.clone());

                hash_to_slot.insert(new_block_0.id, Slot::new(i, 0));
                hash_to_slot.insert(new_block_1.id, Slot::new(i, 1));
                hash_to_slot.insert(new_block_2.id, Slot::new(i, 2));
                hash_to_slot.insert(new_block_3.id, Slot::new(i, 3));

                prev_blocks = vec![
                    new_block_0.id,
                    new_block_1.id,
                    new_block_2.id,
                    new_block_3.id,
                ];
                std::thread::sleep(Duration::from_millis(1000));
            }

            // Should still have 5 max cliques now.
            let mut status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                5,
                "incorrect number of max cliques"
            );

            // Period 7
            // Max clique 1.
            let new_block_0 = create_block(
                Slot::new(7, 0),
                vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                &staking_key,
            );
            register_block(&consensus_controller, new_block_0.clone(), storage.clone());
            let new_block_1 = create_block(
                Slot::new(7, 1),
                vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                &staking_key,
            );
            register_block(&consensus_controller, new_block_1.clone(), storage.clone());
            // Max clique 2.
            let new_block_2 = create_block(
                Slot::new(7, 2),
                vec![block_2_0.id, block_2_1.id, prev_blocks[2], prev_blocks[3]],
                &staking_key,
            );
            register_block(&consensus_controller, new_block_2.clone(), storage.clone());
            let new_block_3 = create_block(
                Slot::new(7, 3),
                vec![block_2_0.id, block_2_1.id, prev_blocks[2], prev_blocks[3]],
                &staking_key,
            );
            register_block(&consensus_controller, new_block_3.clone(), storage.clone());

            hash_to_slot.insert(new_block_0.id, Slot::new(7, 0));
            hash_to_slot.insert(new_block_1.id, Slot::new(7, 1));
            hash_to_slot.insert(new_block_2.id, Slot::new(7, 2));
            hash_to_slot.insert(new_block_2.id, Slot::new(7, 3));

            std::thread::sleep(Duration::from_millis(1000));
            // Should still have 2 max cliques now.
            status = consensus_controller
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
            for i in 8..=15 {
                // Max clique 1.
                let new_block_0 = create_block(
                    Slot::new(i, 0),
                    vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                    &staking_key,
                );
                register_block(&consensus_controller, new_block_0.clone(), storage.clone());
                let new_block_1 = create_block(
                    Slot::new(i, 1),
                    vec![prev_blocks[0], prev_blocks[1], block_1_2.id, block_1_3.id],
                    &staking_key,
                );
                register_block(&consensus_controller, new_block_1.clone(), storage.clone());
                // Max clique 2.
                let new_block_2 = create_block(
                    Slot::new(i, 2),
                    vec![block_2_0.id, block_2_1.id, prev_blocks[2], prev_blocks[3]],
                    &staking_key,
                );
                register_block(&consensus_controller, new_block_2.clone(), storage.clone());
                let new_block_3 = create_block(
                    Slot::new(i, 3),
                    vec![block_2_0.id, block_2_1.id, prev_blocks[2], prev_blocks[3]],
                    &staking_key,
                );
                register_block(&consensus_controller, new_block_3.clone(), storage.clone());

                hash_to_slot.insert(new_block_0.id, Slot::new(i, 0));
                hash_to_slot.insert(new_block_1.id, Slot::new(i, 1));
                hash_to_slot.insert(new_block_2.id, Slot::new(i, 2));
                hash_to_slot.insert(new_block_3.id, Slot::new(i, 3));

                prev_blocks = vec![
                    new_block_0.id,
                    new_block_1.id,
                    new_block_2.id,
                    new_block_3.id,
                ];
                std::thread::sleep(Duration::from_millis(1000));
            }

            // Should still have 3 max cliques now.
            status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                2,
                "incorrect number of max cliques"
            );
        },
    );
}
