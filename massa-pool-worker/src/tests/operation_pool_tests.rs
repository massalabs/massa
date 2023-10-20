// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! # Internal Pool units tests
//! Units tests that are internals to pool and do not require any foreign
//! modules to be tested.
//!
//! # Add operations
//! Function: [`test_add_operation`]
//! Classic usage of internal `add_operations` function from the [`OperationPool`].
//!
//! # Add irrelevant operation
//! Function: [`test_add_irrelevant_operation`]
//! Same as classic but we try to add irrelevant operation. (See the definition
//! chapter below)
//!
//! # Definition
//! Relevant operation: Operation with a validity range corresponding to the
//! latest period given his own thread. All operation which doesn't fit these
//! requirements are "irrelevant"
//!
use crate::tests::tools::OpGenerator;

use super::tools::{
    create_some_operations, default_mock_execution_controller, pool_test, PoolTestBoilerPlate,
};
use massa_models::{amount::Amount, config::ENDORSEMENT_COUNT, operation::OperationId, slot::Slot};
use massa_pool_exports::PoolConfig;
use massa_pos_exports::{MockSelectorController, Selection};
use std::{collections::BTreeMap, time::Duration};

#[test]
fn test_add_operation() {
    let execution_controller = default_mock_execution_controller();
    let selector_controller = {
        let mut res = Box::new(MockSelectorController::new());
        res.expect_clone_box().times(2).returning(|| {
            let mut story = MockSelectorController::new();
            story
                .expect_get_available_selections_in_range()
                .returning(|slot_range, opt_addrs| {
                    let mut all_slots = BTreeMap::new();
                    let addr = *opt_addrs
                        .expect("No addresses filter given")
                        .iter()
                        .next()
                        .expect("No addresses given");
                    for i in 0..15 {
                        for j in 0..32 {
                            let s = Slot::new(i, j);
                            if slot_range.contains(&s) {
                                all_slots.insert(
                                    s,
                                    Selection {
                                        producer: addr,
                                        endorsements: vec![addr; ENDORSEMENT_COUNT as usize],
                                    },
                                );
                            }
                        }
                    }
                    Ok(all_slots)
                });
            Box::new(story)
        });
        res
    };
    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        None,
        |mut operation_pool, mut storage| {
            let op_gen = OpGenerator::default().expirery(2);
            storage.store_operations(create_some_operations(10, &op_gen));
            operation_pool.add_operations(storage);
            // Allow some time for the pool to add the operations
            std::thread::sleep(Duration::from_secs(3));
            assert_eq!(operation_pool.get_operation_count(), 10);
        },
    );
}

/// Test if adding irrelevant operations make simply skip the add.
/// # Initialization
#[test]
fn test_add_irrelevant_operation() {
    let pool_config = PoolConfig::default();
    let thread_count = pool_config.thread_count;
    let execution_controller = default_mock_execution_controller();
    let selector_controller = {
        let mut res = Box::new(MockSelectorController::new());
        res.expect_clone_box().times(2).returning(|| {
            let mut story = MockSelectorController::new();
            story
                .expect_get_available_selections_in_range()
                .returning(|slot_range, opt_addrs| {
                    let mut all_slots = BTreeMap::new();
                    let addr = *opt_addrs
                        .expect("No addresses filter given")
                        .iter()
                        .next()
                        .expect("No addresses given");
                    for i in 0..15 {
                        for j in 0..32 {
                            let s = Slot::new(i, j);
                            if slot_range.contains(&s) {
                                all_slots.insert(
                                    s,
                                    Selection {
                                        producer: addr,
                                        endorsements: vec![addr; ENDORSEMENT_COUNT as usize],
                                    },
                                );
                            }
                        }
                    }
                    Ok(all_slots)
                });
            Box::new(story)
        });
        res
    };
    pool_test(
        PoolConfig::default(),
        execution_controller,
        selector_controller,
        None,
        |mut operation_pool, mut storage| {
            let op_gen = OpGenerator::default().expirery(2);
            storage.store_operations(create_some_operations(10, &op_gen));
            operation_pool.notify_final_cs_periods(&vec![51; thread_count.into()]);
            operation_pool.add_operations(storage);
            // Allow some time for the pool to add the operations
            std::thread::sleep(Duration::from_secs(3));
            assert_eq!(operation_pool.get_operation_count(), 0);
        },
    );
}

#[test]
fn test_pool() {
    let mut pool_config = PoolConfig::default();
    pool_config.max_operations_per_block = 10;
    let execution_controller = default_mock_execution_controller();
    let selector_controller = {
        let mut res = Box::new(MockSelectorController::new());
        res.expect_clone_box().times(2).returning(|| {
            let mut story = MockSelectorController::new();
            story
                .expect_get_available_selections_in_range()
                .returning(|slot_range, opt_addrs| {
                    let mut all_slots = BTreeMap::new();
                    let addr = *opt_addrs
                        .expect("No addresses filter given")
                        .iter()
                        .next()
                        .expect("No addresses given");
                    for i in 0..15 {
                        for j in 0..32 {
                            let s = Slot::new(i, j);
                            if slot_range.contains(&s) {
                                all_slots.insert(
                                    s,
                                    Selection {
                                        producer: addr,
                                        endorsements: vec![addr; ENDORSEMENT_COUNT as usize],
                                    },
                                );
                            }
                        }
                    }
                    Ok(all_slots)
                });
            Box::new(story)
        });
        res
    };
    let PoolTestBoilerPlate {
        mut pool_manager,
        mut pool_controller,
        storage: storage_base,
    } = PoolTestBoilerPlate::pool_test(pool_config, execution_controller, selector_controller);

    // // generate (id, transactions, range of validity) by threads
    let mut thread_tx_lists = vec![Vec::new(); pool_config.thread_count as usize];

    let mut storage = storage_base.clone_without_refs();
    for i in 0..500 {
        let expire_period = 3;
        let op = OpGenerator::default()
            .expirery(expire_period)
            .fee(Amount::const_init(1 + i, 3)) // can panic but not a big deal as we are testing
            .generate(); //get_transaction(expire_period, fee);

        storage.store_operations(vec![op.clone()]);
        let op_thread = op
            .content_creator_address
            .get_thread(pool_config.thread_count);

        let start_period = expire_period.saturating_sub(pool_config.operation_validity_periods);

        thread_tx_lists[op_thread as usize].push((op, start_period..=expire_period));
    }

    pool_controller.add_operations(storage);
    std::thread::sleep(Duration::from_secs(3));
    // // sort from bigger fee to smaller and truncate
    for lst in thread_tx_lists.iter_mut() {
        lst.reverse();
        lst.truncate(pool_config.max_operations_per_block as usize);
    }

    // // checks ops are the expected ones for thread 0 and 1 and various periods
    for thread in 0u8..pool_config.thread_count {
        let target_slot = Slot::new(0, thread);
        let (ids, storage) = pool_controller.get_block_operations(&target_slot);

        assert_eq!(
            ids.iter()
                .map(|id| (
                    *id,
                    storage
                        .read_operations()
                        .get(id)
                        .unwrap()
                        .serialized_data
                        .clone()
                ))
                .collect::<Vec<(OperationId, Vec<u8>)>>(),
            thread_tx_lists[target_slot.thread as usize]
                .iter()
                .filter(|(_, r)| r.contains(&target_slot.period))
                .map(|(op, _)| (op.id, op.serialized_data.clone()))
                .collect::<Vec<(OperationId, Vec<u8>)>>()
        );
    }
    pool_manager.stop();
}
