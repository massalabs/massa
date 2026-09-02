use std::{
    collections::{HashSet, VecDeque},
    time::Duration,
};

use super::{
    tools::{consensus_test, register_block},
    universe::{ConsensusForeignControllers, ConsensusTestUniverse},
};
use crate::tests::tools::create_block;
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
use massa_test_framework::TestUniverse;
use massa_time::MassaTime;
use mockall::Sequence;

#[test]
fn test_genesis_block_creation() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let thread_count = 2;
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(1000),
        thread_count,
        genesis_timestamp: MassaTime::now(),
        force_keep_final_periods: 50,
        force_keep_final_periods_without_ops: 128,
        max_future_processing_blocks: 10,
        genesis_key: staking_key.clone(),
        ..ConsensusConfig::default()
    };
    let mut foreign_controllers = ConsensusForeignControllers::new_with_mocks();

    foreign_controllers
        .execution_controller
        .expect_update_blockclique_status()
        .return_once(|_, _, _| {});
    foreign_controllers
        .pool_controller
        .expect_notify_final_cs_periods()
        .returning(|_| {});
    foreign_controllers
        .pool_controller
        .expect_add_denunciation_precursor()
        .returning(|_| {});
    let universe = ConsensusTestUniverse::new(foreign_controllers, cfg);
    let genesis_hashes = universe
        .module_controller
        .get_block_graph_status(None, None)
        .expect("could not get block graph status")
        .genesis_blocks;
    assert_eq!(genesis_hashes.len() as u8, thread_count);
}

/// This test tests that the blocks are well processed by consensus even if they are not sent in a sorted way.
#[test]
fn test_unsorted_block() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(1000),
        thread_count: 2,
        genesis_timestamp: MassaTime::now(),
        force_keep_final_periods: 50,
        force_keep_final_periods_without_ops: 128,
        max_future_processing_blocks: 10,
        genesis_key: staking_key.clone(),
        ..ConsensusConfig::default()
    };

    let start_period = 3;
    let sent_slot_order = vec![
        Slot::new(1 + start_period, 0),
        Slot::new(1 + start_period, 1),
        Slot::new(3 + start_period, 0),
        Slot::new(4 + start_period, 1),
        Slot::new(4 + start_period, 0),
        Slot::new(2 + start_period, 0),
        Slot::new(3 + start_period, 1),
        Slot::new(2 + start_period, 1),
    ];
    let staking_address = Address::from_public_key(&staking_key.get_public_key());

    let mut foreign_controllers = ConsensusForeignControllers::new_with_mocks();
    let storage = foreign_controllers.storage.clone();

    foreign_controllers
        .execution_controller
        .expect_update_blockclique_status()
        .return_once(|_, _, _| {});
    //TODO: Improve checks here
    foreign_controllers
        .pool_controller
        .expect_notify_final_cs_periods()
        .returning(|_| {});
    foreign_controllers
        .pool_controller
        .expect_add_denunciation_precursor()
        .returning(|_| {});
    let mut sequence = Sequence::new();
    for slot in sent_slot_order.into_iter() {
        foreign_controllers
            .selector_controller
            .expect_get_producer()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(move |sent_slot| {
                assert_eq!(sent_slot, slot);
                Ok(staking_address)
            });
    }
    let universe = ConsensusTestUniverse::new(foreign_controllers, cfg);
    let genesis_hashes = universe
        .module_controller
        .get_block_graph_status(None, None)
        .expect("could not get block graph status")
        .genesis_blocks;
    // create test blocks

    let t0s1 = create_block(
        Slot::new(1 + start_period, 0),
        genesis_hashes.clone(),
        &staking_key,
    );

    let t1s1 = create_block(
        Slot::new(1 + start_period, 1),
        genesis_hashes.clone(),
        &staking_key,
    );

    let t0s2 = create_block(
        Slot::new(2 + start_period, 0),
        vec![t0s1.id, t1s1.id],
        &staking_key,
    );
    let t1s2 = create_block(
        Slot::new(2 + start_period, 1),
        vec![t0s1.id, t1s1.id],
        &staking_key,
    );

    let t0s3 = create_block(
        Slot::new(3 + start_period, 0),
        vec![t0s2.id, t1s2.id],
        &staking_key,
    );
    let t1s3 = create_block(
        Slot::new(3 + start_period, 1),
        vec![t0s2.id, t1s2.id],
        &staking_key,
    );

    let t0s4 = create_block(
        Slot::new(4 + start_period, 0),
        vec![t0s3.id, t1s3.id],
        &staking_key,
    );
    let t1s4 = create_block(
        Slot::new(4 + start_period, 1),
        vec![t0s3.id, t1s3.id],
        &staking_key,
    );

    // send blocks  t0s1, t1s1,
    register_block(&universe.module_controller, t0s1.clone(), storage.clone());
    register_block(&universe.module_controller, t1s1.clone(), storage.clone());
    //register blocks t0s3, t1s4, t0s4, t0s2, t1s3, t1s2
    register_block(&universe.module_controller, t0s3.clone(), storage.clone());
    register_block(&universe.module_controller, t1s4.clone(), storage.clone());
    register_block(&universe.module_controller, t0s4.clone(), storage.clone());
    register_block(&universe.module_controller, t0s2.clone(), storage.clone());
    register_block(&universe.module_controller, t1s3.clone(), storage.clone());
    register_block(&universe.module_controller, t1s2.clone(), storage.clone());
}

#[test]
fn test_parallel_incompatibility() {
    let thread_count = 2;
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(100),
        thread_count,
        genesis_timestamp: MassaTime::now(),
        force_keep_final_periods_without_ops: 128,
        force_keep_final_periods: 10,
        delta_f0: 32,
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

            let block_1 = create_block(Slot::new(1, 0), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_1.clone(), storage.clone());

            let block_2 = create_block(Slot::new(1, 1), vec![genesis[0], genesis[1]], &staking_key);
            register_block(&consensus_controller, block_2.clone(), storage.clone());

            let block_3 = create_block(Slot::new(2, 0), vec![block_1.id, genesis[1]], &staking_key);
            register_block(&consensus_controller, block_3.clone(), storage.clone());

            let block_4 = create_block(Slot::new(2, 1), vec![genesis[0], block_2.id], &staking_key);
            register_block(&consensus_controller, block_4.clone(), storage.clone());

            std::thread::sleep(Duration::from_millis(500));
            let status = consensus_controller
                .get_block_graph_status(None, None)
                .expect("could not get block graph status");

            assert!(if let Some(h) = status.gi_head.get(&block_4.id) {
                h.contains(&block_3.id)
            } else {
                panic!("missing block in gi_head")
            });

            assert_eq!(status.max_cliques.len(), 2);

            for clique in status.max_cliques.clone() {
                if clique.block_ids.contains(&block_3.id) && clique.block_ids.contains(&block_4.id)
                {
                    panic!("incompatible blocks in the same clique")
                }
            }

            assert_eq!(
                &status.best_parents,
                &vec![
                    (block_3.id, block_3.content.header.content.slot.period),
                    (block_2.id, block_2.content.header.content.slot.period),
                ],
                "wrong best_parents"
            );

            let mut latest_extra_blocks = VecDeque::new();
            for extend_i in 0..33 {
                let status = consensus_controller
                    .get_block_graph_status(None, None)
                    .expect("could not get block graph status");
                let block = create_block(
                    Slot::new(3 + extend_i, 0),
                    status.best_parents.iter().map(|(b, _p)| *b).collect(),
                    &staking_key,
                );
                register_block(&consensus_controller, block.clone(), storage.clone());
                std::thread::sleep(Duration::from_millis(100));
                latest_extra_blocks.push_back(block.id);
                while latest_extra_blocks.len() > cfg.delta_f0 as usize + 1 {
                    latest_extra_blocks.pop_front();
                }
            }
            let latest_extra_blocks: HashSet<BlockId> = latest_extra_blocks.into_iter().collect();
            let status = consensus_controller
                .get_block_graph_status(None, None)
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
        },
    );
}

#[test]
fn test_parent_in_the_future() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let cfg = ConsensusConfig {
        t0: MassaTime::from_millis(1000),
        thread_count: 2,
        genesis_timestamp: MassaTime::now(),
        force_keep_final_periods: 50,
        force_keep_final_periods_without_ops: 128,
        max_future_processing_blocks: 2,
        block_db_prune_interval: MassaTime::from_millis(250),
        genesis_key: staking_key.clone(),
        ..ConsensusConfig::default()
    };
    let staking_address = Address::from_public_key(&staking_key.get_public_key());
    let start_period = 100;

    let mut foreign_controllers = ConsensusForeignControllers::new_with_mocks();
    let storage = foreign_controllers.storage.clone();

    foreign_controllers
        .execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    foreign_controllers
        .pool_controller
        .expect_notify_final_cs_periods()
        .returning(|_| {});
    foreign_controllers
        .pool_controller
        .expect_add_denunciation_precursor()
        .returning(|_| {});
    foreign_controllers
        .selector_controller
        .expect_get_producer()
        .returning(move |_| Ok(staking_address));
    foreign_controllers
        .selector_controller
        .expect_get_selection()
        .returning(move |_| {
            Ok(Selection {
                producer: staking_address,
                endorsements: vec![staking_address; ENDORSEMENT_COUNT as usize],
            })
        });

    let universe = ConsensusTestUniverse::new(foreign_controllers, cfg);
    let genesis_hashes = universe
        .module_controller
        .get_block_graph_status(None, None)
        .expect("could not get block graph status")
        .genesis_blocks;
    // create test blocks

    let t0s1 = create_block(
        Slot::new(start_period, 0),
        genesis_hashes.clone(),
        &staking_key,
    );

    let t1s1 = create_block(
        Slot::new(start_period, 1),
        genesis_hashes.clone(),
        &staking_key,
    );

    let t0s2 = create_block(
        Slot::new(start_period.saturating_add(1), 0),
        vec![t0s1.id, t1s1.id],
        &staking_key,
    );

    // send blocks  t0s1, t1s1,
    register_block(&universe.module_controller, t0s1.clone(), storage.clone());
    register_block(&universe.module_controller, t1s1.clone(), storage.clone());
    //register blocks t0s2
    register_block(&universe.module_controller, t0s2.clone(), storage.clone());

    // Wait for blocks_db to be pruned
    std::thread::sleep(Duration::from_millis(1500));
    // We only keep t0s2 and t1s1 because of max_future_processing_blocks set to 2
    let status = universe
        .module_controller
        .get_block_statuses(&[t0s1.id, t1s1.id, t0s2.id]);
    assert_eq!(
        status,
        vec![
            BlockGraphStatus::WaitingForSlot,
            BlockGraphStatus::WaitingForSlot,
            BlockGraphStatus::NotFound
        ],
        "wrong status"
    );
}

/// Regression test for F29: slot-driven maintenance must not be starved by a
/// sustained flood of consensus commands.
///
/// A future block header is registered, then the worker is flooded with
/// cheap `mark_invalid_block` commands. While the flood is still running, the
/// block's slot passes. With the starvation bug the block would stay in
/// `WaitingForSlot` until the flood ends; with the fix `slot_tick` runs
/// between commands and the block leaves `WaitingForSlot` during the flood.
#[test]
fn test_slot_maintenance_not_starved_by_command_flood() {
    let staking_key: KeyPair = KeyPair::generate(0).unwrap();
    let thread_count = 2;
    let t0 = MassaTime::from_millis(100);
    let cfg = ConsensusConfig {
        t0,
        thread_count,
        genesis_timestamp: MassaTime::now(),
        force_keep_final_periods: 50,
        force_keep_final_periods_without_ops: 128,
        max_future_processing_blocks: 10,
        genesis_key: staking_key.clone(),
        ..ConsensusConfig::default()
    };

    let last_start_period = cfg.last_start_period;

    let staking_address = Address::from_public_key(&staking_key.get_public_key());

    let mut foreign_controllers = ConsensusForeignControllers::new_with_mocks();

    // `slot_tick` / `block_db_changed` will call the execution controller
    // multiple times, so use `returning` rather than `return_once`.
    foreign_controllers
        .execution_controller
        .expect_update_blockclique_status()
        .returning(|_, _, _| {});
    foreign_controllers
        .pool_controller
        .expect_notify_final_cs_periods()
        .returning(|_| {});
    foreign_controllers
        .pool_controller
        .expect_add_denunciation_precursor()
        .returning(|_| {});
    foreign_controllers
        .selector_controller
        .expect_get_producer()
        .returning(move |_| Ok(staking_address));
    foreign_controllers
        .selector_controller
        .expect_get_selection()
        .returning(move |_| {
            Ok(Selection {
                producer: staking_address,
                endorsements: vec![staking_address; ENDORSEMENT_COUNT as usize],
            })
        });

    let universe = ConsensusTestUniverse::new(foreign_controllers, cfg);
    let controller = universe.module_controller.clone();

    let genesis_hashes = controller
        .get_block_graph_status(None, None)
        .expect("could not get block graph status")
        .genesis_blocks;

    // Register a block header whose slot is far enough in the future that
    // the test can flood commands across its slot boundary.
    let future_block = create_block(
        Slot::new(3 + last_start_period, 0),
        genesis_hashes.clone(),
        &staking_key,
    );
    controller.register_block_header(future_block.id, future_block.content.header.clone());

    // Wait until the block is known and waiting for its slot.
    let mut waited = 0u64;
    loop {
        std::thread::sleep(Duration::from_millis(10));
        waited += 10;
        let status = &controller.get_block_statuses(&[future_block.id])[0];
        if *status == BlockGraphStatus::WaitingForSlot {
            break;
        }
        assert!(
            waited < 1000,
            "future block never reached WaitingForSlot (status: {:?})",
            status
        );
    }

    // Cheap command used to keep the worker busy. The block must exist in the
    // graph before we can mark it invalid, so register its header first.
    let dummy_block = create_block(
        Slot::new(1 + last_start_period, 0),
        genesis_hashes,
        &staking_key,
    );
    controller.register_block_header(dummy_block.id, dummy_block.content.header.clone());

    // Flood the command channel from a separate thread.
    let flood_controller = controller.clone();
    let flood_handle = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_millis(600) {
            flood_controller.mark_invalid_block(dummy_block.id, dummy_block.content.header.clone());
        }
    });

    // Poll the future block status while the flood is still running.
    // With the fix, slot_tick will run during the flood and process the block.
    let mut processed_during_flood = false;
    let poll_start = std::time::Instant::now();
    while poll_start.elapsed() < Duration::from_millis(600) {
        std::thread::sleep(Duration::from_millis(25));
        if &controller.get_block_statuses(&[future_block.id])[0]
            != &BlockGraphStatus::WaitingForSlot
        {
            processed_during_flood = true;
            break;
        }
    }

    flood_handle.join().expect("flood thread panicked");

    assert!(
        processed_during_flood,
        "future block stayed in WaitingForSlot during command flood; slot maintenance was starved"
    );
}
