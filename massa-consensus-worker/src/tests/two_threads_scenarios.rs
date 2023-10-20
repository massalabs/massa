use std::time::Duration;

use massa_consensus_exports::ConsensusConfig;
use massa_execution_exports::MockExecutionController;
use massa_models::{
    address::Address, block::BlockGraphStatus, config::ENDORSEMENT_COUNT, slot::Slot,
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
fn test_tts_latest_blocks_as_parents() {
    let t0_millis: u64 = 200;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(t0_millis),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_notify_final_cs_periods()
            .returning(|_| {});
        pool_controller
    });
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
            let block_1_0 =
                create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 = create_block(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(500));

            // Period 2.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![block_2_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(500));

            // Period 3, thread 0.
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());

            // block_1_0 has not been finalized yet.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
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
                vec![block_3_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(500));

            // block_1_0 has been finalized while block_1_1 has not.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 4, thread 0.
            let block_4_0 = create_block(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(500));
            // block_1_1 has been finalized while block_2_0 has not.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 4, thread 1.
            let block_4_1 = create_block(
                Slot::new(4, 1),
                vec![block_4_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(500));
            // block_2_0 has been finalized while block_2_1 has not.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
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

// Always use latest period blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_tts_latest_period_blocks_as_parents() {
    let t0_millis: u64 = 200;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(t0_millis),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_notify_final_cs_periods()
            .returning(|_| {});
        pool_controller
    });
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
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 1.
            let block_1_0 =
                create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 =
                create_block(Slot::new(1, 1), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 2.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 3.
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());
            let block_3_1 = create_block(
                Slot::new(3, 1),
                vec![block_2_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_1_0 and block_1_1 have not been finalized yet.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 4, thread 0.
            let block_4_0 = create_block(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_1_0 and block_1_1 have been finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 4, thread 1.
            let block_4_1 = create_block(
                Slot::new(4, 1),
                vec![block_3_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(300));

            // No new finalized blocks.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );
        },
    );
}

// Sometimes use latest period blocks while sometimes use latest blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_tts_mixed_blocks_as_parents() {
    let t0_millis: u64 = 200;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(t0_millis),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_notify_final_cs_periods()
            .returning(|_| {});
        pool_controller
    });
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

            std::thread::sleep(Duration::from_millis(t0_millis));
            // Period 1.
            let block_1_0 =
                create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 = create_block(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 2.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 3, thread 0.
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_1_0 has not been finalized yet.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
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
                vec![block_3_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_1_0 has been finalized while block_1_1 has not.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 4, thread 0.
            let block_4_0 = create_block(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(100));

            // block_1_1 has been finalized while block_2_0 has not.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 4, thread 1.
            let block_4_1 = create_block(
                Slot::new(4, 1),
                vec![block_3_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(100));

            // Neither of block_2_0 and block_2_1 have been finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );
        },
    );
}

// A block at period 2 uses a block at period 0 as a parent.
// Other blocks use latest period blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_tts_p2_depends_on_p0_1() {
    let t0_millis: u64 = 200;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(t0_millis),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_notify_final_cs_periods()
            .returning(|_| {});
        pool_controller
    });
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
            let block_1_0 =
                create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 =
                create_block(Slot::new(1, 1), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(400));

            // Period 2.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 3.
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());
            let block_3_1 = create_block(
                Slot::new(3, 1),
                vec![block_2_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 4.
            let block_4_0 = create_block(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_0.clone(), storage.clone());
            let block_4_1 = create_block(
                Slot::new(4, 1),
                vec![block_3_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Neither of block_2_0 and block_2_1 have not been finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 5.
            let block_5_0 = create_block(
                Slot::new(5, 0),
                vec![block_4_0.id, block_4_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_5_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Both of block_2_0 and block_2_1 have been finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::Final,
                ],
                "incorrect block statuses"
            );
        },
    );
}

// A block at period 2 uses a block at period 0 as a parent.
// Other blocks use latest blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_tts_p2_depends_on_p0_2() {
    let t0_millis: u64 = 200;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(t0_millis),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_notify_final_cs_periods()
            .returning(|_| {});
        pool_controller
    });
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
            let block_1_0 =
                create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 = create_block(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(400));

            // Period 2.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![block_2_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 3.
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());
            let block_3_1 = create_block(
                Slot::new(3, 1),
                vec![block_3_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 4.
            let block_4_0 = create_block(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_0.clone(), storage.clone());
            let block_4_1 = create_block(
                Slot::new(4, 1),
                vec![block_4_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_2_0 has been finalized while block_2_1 has not.
            assert_eq!(
                consensus_controller.get_block_statuses(&[block_2_0.id, block_2_1.id]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 5.
            let block_5_0 = create_block(
                Slot::new(5, 0),
                vec![block_4_0.id, block_4_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_5_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_2_1 has been finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[block_2_0.id, block_2_1.id]),
                [BlockGraphStatus::Final, BlockGraphStatus::Final,],
                "incorrect block statuses"
            );
        },
    );
}

// A block at period 3 uses a block at period 0 as a parent.
// Other blocks use latest period blocks as parents.
// The block should be marked as Discarded without introducing new max cliques.
#[test]
fn test_tts_p3_depends_on_p0() {
    let t0_millis: u64 = 200;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(t0_millis),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_notify_final_cs_periods()
            .returning(|_| {});
        pool_controller
    });
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
            std::thread::sleep(Duration::from_millis(t0_millis));
            // Period 1.
            let block_1_0 =
                create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 =
                create_block(Slot::new(1, 1), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 2.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 3, thread 0.
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_3_0 should be discarded without introducing new max cliques.
            assert_eq!(
                consensus_controller.get_block_statuses(&[block_3_0.id]),
                [BlockGraphStatus::Discarded,],
                "incorrect block statuses"
            );
            let status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                1,
                "incorrect number of max cliques"
            );
        },
    );
}

// Multiple blocks use a block at period 0 as a parent.
// Other blocks use latest blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_tts_multiple_blocks_depend_on_p0_no_incomp() {
    let t0_millis: u64 = 200;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(t0_millis),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_notify_final_cs_periods()
            .returning(|_| {});
        pool_controller
    });
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
            std::thread::sleep(Duration::from_millis(t0_millis));
            // Period 1.
            let block_1_0 =
                create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 = create_block(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 2.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![block_2_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 3, thread 0.
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // No blocks are finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
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
                vec![block_3_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_1_0 has been finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 4.
            let block_4_0 = create_block(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_1_1 has not been finalized yet, lagging a bit.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            let block_4_1 = create_block(
                Slot::new(4, 1),
                vec![block_4_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_1_1 and block_2_0 have been finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
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

// Multiple blocks use a block at period 0 as a parent.
// Other blocks use latest blocks as parents.
// Block_2_1 and block_3_0 have a parallel incompatibility, because thread 0 is lagging too much.
#[test]
fn test_tts_multiple_blocks_depend_on_p0_parallel_incomp() {
    let t0_millis: u64 = 200;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(t0_millis),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_notify_final_cs_periods()
            .returning(|_| {});
        pool_controller
    });
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

            std::thread::sleep(Duration::from_millis(t0_millis));
            // Period 1.
            let block_1_0 =
                create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 = create_block(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 2.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![block_2_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Should have one max clique now.
            let mut status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                1,
                "incorrect number of max cliques"
            );

            // Period 3, thread 0.
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Should have two max cliques now.
            status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                2,
                "incorrect number of max cliques"
            );

            // block_2_1 and block_3_0 should be in different max cliques.
            if status.max_cliques[0].block_ids.contains(&block_2_1.id) {
                assert!(
                    status.max_cliques[1].block_ids.contains(&block_3_0.id),
                    "block_2_1 and block_3_0 should not be in the same max clique"
                );
            } else {
                assert!(
                    status.max_cliques[0].block_ids.contains(&block_3_0.id),
                    "block_2_1 and block_3_0 should not be in the same max clique"
                );
            }
        },
    );
}

// A parent of a block is registered later than the time when the block is registered.
// Blocks should be finalized as expected.
#[test]
fn test_tts_parent_registered_later() {
    let t0_millis: u64 = 200;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(t0_millis),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_notify_final_cs_periods()
            .returning(|_| {});
        pool_controller
    });
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

            std::thread::sleep(Duration::from_millis(t0_millis));
            // Period 1, thread 0.
            let block_1_0 =
                create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());

            // Period 1, thread 1.
            // Create block_1_1 but don't register it.
            let block_1_1 =
                create_block(Slot::new(1, 1), vec![genesis[0], genesis[1]], &staking_key);

            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 2, thread 0.
            // Create and register block_2_0.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            // Period 2, thread 1.
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 3.
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());
            let block_3_1 = create_block(
                Slot::new(3, 1),
                vec![block_2_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Period 4.
            let block_4_0 = create_block(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_0.clone(), storage.clone());
            let block_4_1 = create_block(
                Slot::new(4, 1),
                vec![block_3_0.id, block_3_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_4_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_1_1 has been finalized while block_2_0 has not.
            assert_eq!(
                consensus_controller.get_block_statuses(&[block_1_1.id, block_2_0.id,]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 5.
            let block_5_0 = create_block(
                Slot::new(5, 0),
                vec![block_4_0.id, block_4_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_5_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_2_1 has been finalized.
            assert_eq!(
                consensus_controller.get_block_statuses(&[block_1_1.id, block_2_0.id,]),
                [BlockGraphStatus::Final, BlockGraphStatus::Final,],
                "incorrect block statuses"
            );
        },
    );
}

// A block using two incompatible blocks as parents should be marked as Discarded.
#[test]
fn test_tts_incompatible_parents() {
    let t0_millis: u64 = 200;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(t0_millis),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
        ..ConsensusConfig::default()
    };
    let storage = Storage::create_root();
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let mut execution_controller = Box::new(MockExecutionController::new());
    execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_notify_final_cs_periods()
            .returning(|_| {});
        pool_controller
    });
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

            std::thread::sleep(Duration::from_millis(t0_millis));
            // Period 1.
            let block_1_0 =
                create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_0.clone(), storage.clone());
            let block_1_1 =
                create_block(Slot::new(1, 1), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Should have one max clique now.
            let mut status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                1,
                "incorrect number of max cliques"
            );

            // Period 2.
            // Grandpa incompatibility.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, genesis[1]],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_0.clone(), storage.clone());
            let block_2_1 = create_block(
                Slot::new(2, 1),
                vec![genesis[0], block_1_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_2_1.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // Should have two max cliques now.
            status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                2,
                "incorrect number of max cliques"
            );

            // Period 3, thread 0.
            // Incompatible parents block_2_0 and block_2_1.
            let block_3_0 = create_block(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &staking_key,
            );
            register_block(&consensus_controller, block_3_0.clone(), storage.clone());
            std::thread::sleep(Duration::from_millis(t0_millis));

            // block_3_0 should be discarded.
            assert_eq!(
                consensus_controller.get_block_statuses(&[block_3_0.id]),
                [BlockGraphStatus::Discarded,],
                "incorrect block statuses"
            );
        },
    );
}
