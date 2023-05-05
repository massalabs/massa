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

use super::tools::{create_some_operations, operation_pool_test, pool_test};
use massa_execution_exports::test_exports;
use massa_execution_exports::MockExecutionController;
use massa_models::{amount::Amount, operation::OperationId, slot::Slot};
use massa_pool_exports::PoolConfig;
use std::time::Duration;

#[test]
fn test_add_operation() {
    operation_pool_test(PoolConfig::default(), |mut operation_pool, mut storage| {
        let op_gen = OpGenerator::default().expirery(2);
        storage.store_operations(create_some_operations(10, &op_gen));
        operation_pool.add_operations(storage);
        // Allow some time for the pool to add the operations
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(operation_pool.storage.get_op_refs().len(), 10);
    });
}

/// Test if adding irrelevant operations make simply skip the add.
/// # Initialization
#[test]
fn test_add_irrelevant_operation() {
    let pool_config = PoolConfig::default();
    let thread_count = pool_config.thread_count;
    operation_pool_test(PoolConfig::default(), |mut operation_pool, mut storage| {
        let op_gen = OpGenerator::default().expirery(2);
        storage.store_operations(create_some_operations(10, &op_gen));
        operation_pool.notify_final_cs_periods(&vec![51; thread_count.into()]);
        operation_pool.add_operations(storage);
        // Allow some time for the pool to add the operations
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(operation_pool.storage.get_op_refs().len(), 0);
    });
}

/// TODO refactor old tests
#[test]
fn test_pool() {
    let pool_config = PoolConfig::default();
    pool_test(
        pool_config,
        |mut pool_manager, mut pool, execution_receiver, _selector_receiver, storage_base| {
            // generate (id, transactions, range of validity) by threads
            let mut thread_tx_lists = vec![Vec::new(); pool_config.thread_count as usize];

            let mut storage = storage_base.clone_without_refs();
            for i in 0..18 {
                let expire_period: u64 = 10 + i;
                let op = OpGenerator::default()
                    .expirery(expire_period)
                    .fee(Amount::from_raw(40 + i))
                    .generate(); //get_transaction(expire_period, fee);

                storage.store_operations(vec![op.clone()]);

                //TODO: compare
                // assert_eq!(storage.get_op_refs(), &Set::<OperationId>::default());

                // duplicate
                // let mut storage = storage_base.clone_without_refs();
                // storage.store_operations(vec![op.clone()]);
                // pool.add_operations(storage);
                //TODO: compare
                //assert_eq!(storage.get_op_refs(), &ops.keys().copied().collect::<Set<OperationId>>());

                let op_thread = op
                    .content_creator_address
                    .get_thread(pool_config.thread_count);

                let start_period =
                    expire_period.saturating_sub(pool_config.operation_validity_periods);

                thread_tx_lists[op_thread as usize].push((op, start_period..=expire_period));
            }

            pool.add_operations(storage);
            // Allow some time for the pool to add the operations
            std::thread::sleep(Duration::from_millis(200));

            // sort from bigger fee to smaller and truncate
            for lst in thread_tx_lists.iter_mut() {
                lst.reverse();
                lst.truncate(pool_config.max_operation_pool_size_per_thread);
            }

            std::thread::spawn(move || loop {
                match execution_receiver.recv_timeout(Duration::from_millis(2000)) {
                    // forward on the operations
                    Ok(test_exports::MockExecutionControllerMessage::UnexecutedOpsAmong {
                        ops,
                        response_tx,
                        ..
                    }) => {
                        response_tx.send(ops).unwrap();
                    }
                    // we want the operations to be paid for...
                    Ok(
                        test_exports::MockExecutionControllerMessage::GetFinalAndCandidateBalance {
                            response_tx,
                            ..
                        },
                    ) => response_tx
                        .send(vec![(
                            Some(Amount::from_raw(60 * 1_000_000_000)),
                            Some(Amount::from_raw(60 * 1_000_000_000)),
                        )])
                        .unwrap(),
                    _ => {}
                }
            });

            // checks ops are the expected ones for thread 0 and 1 and various periods
            for thread in 0u8..pool_config.thread_count {
                for period in 0u64..70 {
                    let target_slot = Slot::new(period, thread);
                    let (ids, storage) = pool.get_block_operations(&target_slot);

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
            }

            // op ending before or at period 45 won't appear in the block due to incompatible validity range
            // we don't keep them as expected ops
            let final_period = 45u64;
            pool.notify_final_cs_periods(&vec![final_period; pool_config.thread_count as usize]);
            // Wait for pool to manage the above command
            std::thread::sleep(Duration::from_millis(200));
            for lst in thread_tx_lists.iter_mut() {
                lst.retain(|(op, _)| op.content.expire_period > final_period);
            }

            // checks ops are the expected ones for thread 0 and 1 and various periods
            for thread in 0u8..pool_config.thread_count {
                for period in 0u64..70 {
                    let target_slot = Slot::new(period, thread);
                    let max_count = 4;
                    let (ids, storage) = pool.get_block_operations(&target_slot);
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
                            .take(max_count)
                            .map(|(op, _)| (op.id, op.serialized_data.clone()))
                            .collect::<Vec<(OperationId, Vec<u8>)>>()
                    );
                }
            }

            // add transactions with a high fee but too much in the future: should be ignored
            {
                //TODO: update current slot
                //pool.update_current_slot(Slot::new(10, 0));
                let expire_period: u64 = 300;
                let op = OpGenerator::default()
                    .expirery(expire_period)
                    .fee(Amount::from_raw(1000))
                    .generate();
                let mut storage = storage_base.clone_without_refs();
                storage.store_operations(vec![op.clone()]);
                pool.add_operations(storage);
                // Allow some time for the pool to add the operations
                std::thread::sleep(Duration::from_millis(100));
                //TODO: compare
                //assert_eq!(storage.get_op_refs(), &Set::<OperationId>::default());
                let op_thread = op
                    .content_creator_address
                    .get_thread(pool_config.thread_count);
                let (ids, _) = pool.get_block_operations(&Slot::new(
                    expire_period - pool_config.operation_validity_periods - 1,
                    op_thread,
                ));
                assert!(ids.is_empty());
            }
            pool_manager.stop();
        },
    );
}
