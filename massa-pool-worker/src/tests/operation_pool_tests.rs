// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! # Internal Pool units tests
//! Units tests that are internals to pool and do not require any foreign
//! modules to be tested.
//!
//! # Add operations
//! Fonction: [test_add_operation]
//! Classic usage of internal `add_operations` function from the [OperationPool].
//!
//! # Add irrelevant operation
//! Function: [test_add_irrelevant_operation]
//! Same as classic but we try to add irrelevant operation. (See the definition
//! chapter below)
//!
//! # Definition
//! Relevant operation: Operation with a validity range corresponding to the
//! latest period given his own thread. All operation which doesn't fit these
//! requirements are "irrelevant"
//!
use super::tools::{create_some_operations, operation_pool_test};
use crate::operation_pool::OperationPool;
use massa_execution_exports::test_exports::MockExecutionController;
use massa_models::{
    address::Address,
    amount::Amount,
    operation::{Operation, OperationSerializer, OperationType, WrappedOperation},
    prehash::PreHashMap,
    slot::Slot,
    wrapped::WrappedContent,
};
use massa_pool_exports::PoolConfig;
use massa_signature::KeyPair;
use massa_storage::Storage;
use std::str::FromStr;

#[test]
fn test_add_operation() {
    operation_pool_test(PoolConfig::default(), |mut operation_pool, mut storage| {
        storage.store_operations(create_some_operations(10, &KeyPair::generate(), 2));
        operation_pool.add_operations(storage);
        assert_eq!(operation_pool.storage.get_op_refs().len(), 10);
    });
}

/// Test if adding irrelevant operations make simply skip the add.
/// # Initilization
/// Init an
#[test]
fn test_add_irrelevant_operation() {
    let pool_config = PoolConfig::default();
    let thread_count = pool_config.thread_count;
    operation_pool_test(PoolConfig::default(), |mut operation_pool, mut storage| {
        storage.store_operations(create_some_operations(10, &KeyPair::generate(), 1));
        operation_pool.notify_final_cs_periods(&vec![51; thread_count.into()]);
        operation_pool.add_operations(storage);
        assert_eq!(operation_pool.storage.get_op_refs().len(), 0);
    });
}

fn get_transaction(expire_period: u64, fee: u64) -> WrappedOperation {
    let sender_keypair = KeyPair::generate();

    let recv_keypair = KeyPair::generate();

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_keypair.get_public_key()),
        amount: Amount::default(),
    };
    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        op,
        expire_period,
    };
    Operation::new_wrapped(content, OperationSerializer::new(), &sender_keypair).unwrap()
}

/// TODO refacto old tests
#[test]
fn test_pool() {
    let (execution_controller, _execution_receiver) = MockExecutionController::new_with_receiver();
    let pool_config = PoolConfig::default();
    let storage_base = Storage::create_root();
    let mut pool = OperationPool::init(pool_config.clone(), &storage_base, execution_controller);
    // generate (id, transactions, range of validity) by threads
    let mut thread_tx_lists = vec![Vec::new(); pool_config.thread_count as usize];
    for i in 0..18 {
        let fee = 40 + i;
        let expire_period: u64 = 40 + i;
        let start_period = expire_period.saturating_sub(pool_config.operation_validity_periods);
        let op = get_transaction(expire_period, fee);
        let id = op.id;

        let mut ops = PreHashMap::default();
        ops.insert(id, op.clone());
        let mut storage = storage_base.clone_without_refs();
        storage.store_operations(ops.values().cloned().collect());
        pool.add_operations(storage);
        //TODO: compare
        // assert_eq!(storage.get_op_refs(), &Set::<OperationId>::default());

        // duplicate
        let mut storage = storage_base.clone_without_refs();
        storage.store_operations(ops.values().cloned().collect());
        pool.add_operations(storage);
        //TODO: compare
        //assert_eq!(storage.get_op_refs(), &ops.keys().copied().collect::<Set<OperationId>>());

        let op_thread = op.creator_address.get_thread(pool_config.thread_count);
        thread_tx_lists[op_thread as usize].push((op, start_period..=expire_period));
    }

    // sort from bigger fee to smaller and truncate
    for lst in thread_tx_lists.iter_mut() {
        lst.reverse();
        lst.truncate(pool_config.max_operation_pool_size_per_thread as usize);
    }

    // checks ops are the expected ones for thread 0 and 1 and various periods
    for thread in 0u8..pool_config.thread_count {
        for period in 0u64..70 {
            let target_slot = Slot::new(period, thread);
            let max_count = 3;
            let (ids, storage) = pool.get_block_operations(&target_slot);
            assert!(ids
                .iter()
                .map(|id| (
                    *id,
                    storage
                        .read_operations()
                        .get(id)
                        .unwrap()
                        .serialized_data
                        .clone()
                ))
                .eq(thread_tx_lists[target_slot.thread as usize]
                    .iter()
                    .filter(|(_, r)| r.contains(&target_slot.period))
                    .take(max_count)
                    .map(|(op, _)| (op.id, op.serialized_data.clone()))));
        }
    }

    // op ending before or at period 45 won't appear in the block due to incompatible validity range
    // we don't keep them as expected ops
    let final_period = 45u64;
    pool.notify_final_cs_periods(&vec![final_period; pool_config.thread_count as usize]);
    for lst in thread_tx_lists.iter_mut() {
        lst.retain(|(op, _)| op.content.expire_period > final_period);
    }

    // checks ops are the expected ones for thread 0 and 1 and various periods
    for thread in 0u8..pool_config.thread_count {
        for period in 0u64..70 {
            let target_slot = Slot::new(period, thread);
            let max_count = 4;
            let (ids, storage) = pool.get_block_operations(&target_slot);
            assert!(ids
                .iter()
                .map(|id| (
                    *id,
                    storage
                        .read_operations()
                        .get(id)
                        .unwrap()
                        .serialized_data
                        .clone()
                ))
                .eq(thread_tx_lists[target_slot.thread as usize]
                    .iter()
                    .filter(|(_, r)| r.contains(&target_slot.period))
                    .take(max_count)
                    .map(|(op, _)| (op.id, op.serialized_data.clone()))));
        }
    }

    // add transactions with a high fee but too much in the future: should be ignored
    {
        //TODO: update current slot
        //pool.update_current_slot(Slot::new(10, 0));
        let fee = 1000;
        let expire_period: u64 = 300;
        let op = get_transaction(expire_period, fee);
        let mut storage = Storage::create_root();
        storage.store_operations(vec![op.clone()]);
        pool.add_operations(storage);
        //TODO: compare
        //assert_eq!(storage.get_op_refs(), &Set::<OperationId>::default());
        let op_thread = op.creator_address.get_thread(pool_config.thread_count);
        let (ids, _) = pool.get_block_operations(&Slot::new(expire_period - 1, op_thread));
        assert!(ids.is_empty());
    }
}
