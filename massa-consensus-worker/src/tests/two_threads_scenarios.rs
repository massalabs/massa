use massa_consensus_exports::ConsensusConfig;
use massa_models::{address::Address, block::BlockGraphStatus, slot::Slot};
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;

use super::tools::{
    answer_ask_producer_pos, answer_ask_selection_pos, consensus_without_pool_test, create_block,
    register_block, register_block_and_process_with_tc, TestController,
};

// Always use latest blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_tts_latest_blocks_as_parents() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(200),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
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
                vec![genesis[0], genesis[1]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1]],
                &tc,
            );

            // Period 2.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id],
                &tc,
            );
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_2_0.id, block_1_1.id],
                &tc,
            );

            // Period 3, thread 0.
            let block_3_0 = register_block_and_process_with_tc(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &tc,
            );

            // block_1_0 has not been finalized yet.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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
            let block_3_1 = register_block_and_process_with_tc(
                Slot::new(3, 1),
                vec![block_3_0.id, block_2_1.id],
                &tc,
            );

            // block_1_0 has been finalized while block_1_1 has not.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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
            let block_4_0 = register_block_and_process_with_tc(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &tc,
            );

            // block_1_1 has been finalized while block_2_0 has not.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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
            let _block_4_1 = register_block_and_process_with_tc(
                Slot::new(4, 1),
                vec![block_4_0.id, block_3_1.id],
                &tc,
            );

            // block_2_0 has been finalized while block_2_1 has not.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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

// Always use latest period blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_tts_latest_period_blocks_as_parents() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(200),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
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
                vec![genesis[0], genesis[1]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![genesis[0], genesis[1]],
                &tc,
            );

            // Period 2.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id],
                &tc,
            );
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id],
                &tc,
            );

            // Period 3.
            let block_3_0 = register_block_and_process_with_tc(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &tc,
            );
            let block_3_1 = register_block_and_process_with_tc(
                Slot::new(3, 1),
                vec![block_2_0.id, block_2_1.id],
                &tc,
            );

            // block_1_0 and block_1_1 have not been finalized yet.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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
            let _block_4_0 = register_block_and_process_with_tc(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &tc,
            );

            // block_1_0 and block_1_1 have been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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
            let _block_4_1 = register_block_and_process_with_tc(
                Slot::new(4, 1),
                vec![block_3_0.id, block_3_1.id],
                &tc,
            );

            // No new finalized blocks.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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

// Sometimes use latest period blocks while sometimes use latest blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_tts_mixed_blocks_as_parents() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(200),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
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
                vec![genesis[0], genesis[1]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1]],
                &tc,
            );

            // Period 2.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id],
                &tc,
            );
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id],
                &tc,
            );

            // Period 3, thread 0.
            let block_3_0 = register_block_and_process_with_tc(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &tc,
            );

            // block_1_0 has not been finalized yet.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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
            let block_3_1 = register_block_and_process_with_tc(
                Slot::new(3, 1),
                vec![block_3_0.id, block_2_1.id],
                &tc,
            );

            // block_1_0 has been finalized while block_1_1 has not.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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
            let _block_4_0 = register_block_and_process_with_tc(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &tc,
            );

            // block_1_1 has been finalized while block_2_0 has not.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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
            let _block_4_1 = register_block_and_process_with_tc(
                Slot::new(4, 1),
                vec![block_3_0.id, block_3_1.id],
                &tc,
            );

            // Neither of block_2_0 and block_2_1 have been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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

// A block at period 2 uses a block at period 0 as a parent.
// Other blocks use latest period blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_tts_p2_depends_on_p0_1() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(200),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
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
                vec![genesis[0], genesis[1]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![genesis[0], genesis[1]],
                &tc,
            );

            // Period 2.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, genesis[1]],
                &tc,
            );
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id],
                &tc,
            );

            // Period 3.
            let block_3_0 = register_block_and_process_with_tc(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &tc,
            );
            let block_3_1 = register_block_and_process_with_tc(
                Slot::new(3, 1),
                vec![block_2_0.id, block_2_1.id],
                &tc,
            );

            // Period 4.
            let block_4_0 = register_block_and_process_with_tc(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &tc,
            );
            let block_4_1 = register_block_and_process_with_tc(
                Slot::new(4, 1),
                vec![block_3_0.id, block_3_1.id],
                &tc,
            );

            // Neither of block_2_0 and block_2_1 have not been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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
            let _block_5_0 = register_block_and_process_with_tc(
                Slot::new(5, 0),
                vec![block_4_0.id, block_4_1.id],
                &tc,
            );

            // Both of block_2_0 and block_2_1 have been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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

// A block at period 2 uses a block at period 0 as a parent.
// Other blocks use latest blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_tts_p2_depends_on_p0_2() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(200),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
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
                vec![genesis[0], genesis[1]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1]],
                &tc,
            );

            // Period 2.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, genesis[1]],
                &tc,
            );
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_2_0.id, block_1_1.id],
                &tc,
            );

            // Period 3.
            let block_3_0 = register_block_and_process_with_tc(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &tc,
            );
            let block_3_1 = register_block_and_process_with_tc(
                Slot::new(3, 1),
                vec![block_3_0.id, block_2_1.id],
                &tc,
            );

            // Period 4.
            let block_4_0 = register_block_and_process_with_tc(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &tc,
            );
            let block_4_1 = register_block_and_process_with_tc(
                Slot::new(4, 1),
                vec![block_4_0.id, block_3_1.id],
                &tc,
            );

            // block_2_0 has been finalized while block_2_1 has not.
            assert_eq!(
                tc.consensus_controller
                    .get_block_statuses(&[block_2_0.id, block_2_1.id]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 5.
            let _block_5_0 = register_block_and_process_with_tc(
                Slot::new(5, 0),
                vec![block_4_0.id, block_4_1.id],
                &tc,
            );

            // block_2_1 has been finalized.
            assert_eq!(
                tc.consensus_controller
                    .get_block_statuses(&[block_2_0.id, block_2_1.id]),
                [BlockGraphStatus::Final, BlockGraphStatus::Final,],
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

// A block at period 3 uses a block at period 0 as a parent.
// Other blocks use latest period blocks as parents.
// The block should be marked as Discarded without introducing new max cliques.
#[test]
fn test_tts_p3_depends_on_p0() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(200),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
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
                vec![genesis[0], genesis[1]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![genesis[0], genesis[1]],
                &tc,
            );

            // Period 2.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id],
                &tc,
            );
            let _block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id],
                &tc,
            );

            // Period 3, thread 0.
            let block_3_0 =
                create_block(Slot::new(3, 0), vec![block_2_0.id, genesis[1]], &tc.creator);
            register_block(
                &tc.consensus_controller,
                &tc.selector_receiver,
                block_3_0.clone(),
                tc.storage.clone(),
            );
            answer_ask_producer_pos(&tc.selector_receiver, &tc.staking_address, 1000);

            // block_3_0 should be discarded without introducing new max cliques.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[block_3_0.id]),
                [BlockGraphStatus::Discarded,],
                "incorrect block statuses"
            );
            let status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                1,
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

// Multiple blocks use a block at period 0 as a parent.
// Other blocks use latest blocks as parents.
// Blocks should be finalized as expected.
#[test]
fn test_tts_multiple_blocks_depend_on_p0_no_incomp() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(200),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
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
                vec![genesis[0], genesis[1]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1]],
                &tc,
            );

            // Period 2.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, genesis[1]],
                &tc,
            );
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_2_0.id, block_1_1.id],
                &tc,
            );

            // Period 3, thread 0.
            let block_3_0 = register_block_and_process_with_tc(
                Slot::new(3, 0),
                vec![block_2_0.id, genesis[1]],
                &tc,
            );

            // No blocks are finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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

            // Period 3, thread 0.
            let block_3_1 = register_block_and_process_with_tc(
                Slot::new(3, 1),
                vec![block_3_0.id, block_2_1.id],
                &tc,
            );

            // block_1_0 has been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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
            let block_4_0 = register_block_and_process_with_tc(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &tc,
            );
            let block_4_1 = register_block_and_process_with_tc(
                Slot::new(4, 1),
                vec![block_4_0.id, block_3_1.id],
                &tc,
            );

            // block_2_0 has been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
                    block_1_0.id,
                    block_1_1.id,
                    block_2_0.id,
                    block_2_1.id
                ]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 5.
            let _block_5_0 = register_block_and_process_with_tc(
                Slot::new(5, 0),
                vec![block_4_0.id, block_4_1.id],
                &tc,
            );

            // block_1_1 has been finalized.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[
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

// Multiple blocks use a block at period 0 as a parent.
// Other blocks use latest blocks as parents.
// block_2_1 and block_4_0 are grandpa incompatibilities.
#[test]
fn test_tts_multiple_blocks_depend_on_p0_grandpa_incomp() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(200),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
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
                vec![genesis[0], genesis[1]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![block_1_0.id, genesis[1]],
                &tc,
            );

            // Period 2.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, genesis[1]],
                &tc,
            );
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_2_0.id, block_1_1.id],
                &tc,
            );

            // Period 3, thread 0.
            let block_3_0 = register_block_and_process_with_tc(
                Slot::new(3, 0),
                vec![block_2_0.id, genesis[1]],
                &tc,
            );

            // Period 3, thread 0.
            let _block_3_1 = register_block_and_process_with_tc(
                Slot::new(3, 1),
                vec![block_3_0.id, block_2_1.id],
                &tc,
            );

            // Should have one max clique now.
            let mut status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                1,
                "incorrect number of max cliques"
            );

            // Period 4, thread 0.
            let block_4_0 = register_block_and_process_with_tc(
                Slot::new(4, 0),
                vec![block_3_0.id, genesis[1]],
                &tc,
            );

            // Should have two max cliques now.
            status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                2,
                "incorrect number of max cliques"
            );

            // block_2_1 and block_4_0 should be in different max cliques.
            if status.max_cliques[0].block_ids.contains(&block_2_1.id) {
                assert_eq!(
                    status.max_cliques[1].block_ids.contains(&block_4_0.id),
                    true,
                    "block_2_1 and block_4_0 should not be in the same max clique"
                );
            } else {
                assert_eq!(
                    status.max_cliques[0].block_ids.contains(&block_4_0.id),
                    true,
                    "block_2_1 and block_4_0 should not be in the same max clique"
                );
            }

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

// A parent of a block is registered later than the time when the block is registered.
// Blocks should be finalized as expected.
#[test]
fn test_tts_parent_registered_later() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(200),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
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

            // Period 1, thread 0.
            let block_1_0 = register_block_and_process_with_tc(
                Slot::new(1, 0),
                vec![genesis[0], genesis[1]],
                &tc,
            );

            // Period 1, thread 1.
            // Create block_1_1 but don't register it.
            let block_1_1 =
                create_block(Slot::new(1, 1), vec![genesis[0], genesis[1]], &tc.creator);

            // Period 2, thread 0.
            // Create and register block_2_0.
            let block_2_0 = create_block(
                Slot::new(2, 0),
                vec![block_1_0.id, block_1_1.id],
                &tc.creator,
            );
            register_block(
                &tc.consensus_controller,
                &tc.selector_receiver,
                block_2_0.clone(),
                tc.storage.clone(),
            );

            // Register block_1_1.
            register_block(
                &tc.consensus_controller,
                &tc.selector_receiver,
                block_1_1.clone(),
                tc.storage.clone(),
            );
            answer_ask_producer_pos(&tc.selector_receiver, &tc.staking_address, 1000);
            answer_ask_selection_pos(&tc.selector_receiver, &tc.staking_address, 1000);
            answer_ask_producer_pos(&tc.selector_receiver, &tc.staking_address, 1000);
            answer_ask_selection_pos(&tc.selector_receiver, &tc.staking_address, 1000);

            // Period 2, thread 1.
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![block_1_0.id, block_1_1.id],
                &tc,
            );

            // Period 3.
            let block_3_0 = register_block_and_process_with_tc(
                Slot::new(3, 0),
                vec![block_2_0.id, block_2_1.id],
                &tc,
            );
            let block_3_1 = register_block_and_process_with_tc(
                Slot::new(3, 1),
                vec![block_2_0.id, block_2_1.id],
                &tc,
            );

            // Period 4.
            let block_4_0 = register_block_and_process_with_tc(
                Slot::new(4, 0),
                vec![block_3_0.id, block_3_1.id],
                &tc,
            );
            let block_4_1 = register_block_and_process_with_tc(
                Slot::new(4, 1),
                vec![block_3_0.id, block_3_1.id],
                &tc,
            );

            // block_1_1 has been finalized while block_2_0 has not.
            assert_eq!(
                tc.consensus_controller
                    .get_block_statuses(&[block_1_1.id, block_2_0.id,]),
                [
                    BlockGraphStatus::Final,
                    BlockGraphStatus::ActiveInBlockclique,
                ],
                "incorrect block statuses"
            );

            // Period 5.
            let _block_5_0 = register_block_and_process_with_tc(
                Slot::new(5, 0),
                vec![block_4_0.id, block_4_1.id],
                &tc,
            );

            // block_2_1 has been finalized.
            assert_eq!(
                tc.consensus_controller
                    .get_block_statuses(&[block_1_1.id, block_2_0.id,]),
                [BlockGraphStatus::Final, BlockGraphStatus::Final,],
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

// A block using two incompatible blocks as parents should be marked as Discarded.
#[test]
fn test_tts_incompatible_parents() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(200),
        thread_count: 2,
        genesis_timestamp: MassaTime::now().unwrap(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 4,
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
                vec![genesis[0], genesis[1]],
                &tc,
            );
            let block_1_1 = register_block_and_process_with_tc(
                Slot::new(1, 1),
                vec![genesis[0], genesis[1]],
                &tc,
            );

            // Should have one max clique now.
            let mut status = tc
                .consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");
            assert_eq!(
                status.max_cliques.len(),
                1,
                "incorrect number of max cliques"
            );

            // Period 2.
            // Grandpa incompatibility.
            let block_2_0 = register_block_and_process_with_tc(
                Slot::new(2, 0),
                vec![block_1_0.id, genesis[1]],
                &tc,
            );
            let block_2_1 = register_block_and_process_with_tc(
                Slot::new(2, 1),
                vec![genesis[0], block_1_1.id],
                &tc,
            );

            // Should have two max cliques now.
            status = tc
                .consensus_controller
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
                &tc.creator,
            );
            register_block(
                &tc.consensus_controller,
                &tc.selector_receiver,
                block_3_0.clone(),
                tc.storage.clone(),
            );
            answer_ask_producer_pos(&tc.selector_receiver, &tc.staking_address, 1000);

            // block_3_0 should be discarded.
            assert_eq!(
                tc.consensus_controller.get_block_statuses(&[block_3_0.id]),
                [BlockGraphStatus::Discarded,],
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
