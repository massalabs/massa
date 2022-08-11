use crate::operation_pool::OperationPool;
use massa_execution_exports::test_exports::MockExecutionController;
use massa_models::{
    prehash::{Map, Set},
    wrapped::WrappedContent,
    Address, Amount, Operation, OperationId, OperationSerializer, OperationType, Slot,
    WrappedOperation,
};
use massa_signature::KeyPair;
use massa_storage::Storage;
use serial_test::serial;
use std::str::FromStr;

use super::config::POOL_CONFIG;

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

#[test]
#[serial]
fn test_pool() {
    let (execution_controller, execution_receiver) = MockExecutionController::new_with_receiver();
    let mut pool = OperationPool::init(*POOL_CONFIG, Default::default(), execution_controller);
    let mut storage = Storage::default();
    // generate (id, transactions, range of validity) by threads
    let mut thread_tx_lists = vec![Vec::new(); POOL_CONFIG.thread_count as usize];
    for i in 0..18 {
        let fee = 40 + i;
        let expire_period: u64 = 40 + i;
        let start_period = expire_period.saturating_sub(POOL_CONFIG.operation_validity_periods);
        let op = get_transaction(expire_period, fee);
        let id = op.verify_integrity().unwrap();

        let mut ops = Map::default();
        ops.insert(id, op.clone());

        storage.store_operations(ops.values().cloned().collect());
        pool.add_operations(storage);
        assert_eq!(storage.get_op_refs(), &Set::<OperationId>::default());

        // duplicate
        storage.store_operations(ops.values().cloned().collect());
        let newly_added = pool.add_operations(storage);
        assert_eq!(storage.get_op_refs(), ops.keys().collect());

        thread_tx_lists[op.thread as usize].push((op, start_period..=expire_period));
    }

    // sort from bigger fee to smaller and truncate
    for lst in thread_tx_lists.iter_mut() {
        lst.reverse();
        lst.truncate(POOL_CONFIG.max_operation_pool_size_per_thread as usize);
    }

    // checks ops are the expected ones for thread 0 and 1 and various periods
    for thread in 0u8..=1 {
        for period in 0u64..70 {
            let target_slot = Slot::new(period, thread);
            let max_count = 3;
            let (ids, storage) = pool.get_block_operations(&target_slot);
            assert!(ids
                .iter()
                .map(|id| (*id, storage.retrieve_operation(id).unwrap().serialized_data))
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
    pool.notify_final_cs_periods(&vec![final_period; POOL_CONFIG.thread_count as usize]);
    for lst in thread_tx_lists.iter_mut() {
        lst.retain(|(op, _)| op.content.expire_period > final_period);
    }

    // checks ops are the expected ones for thread 0 and 1 and various periods
    for thread in 0u8..=1 {
        for period in 0u64..70 {
            let target_slot = Slot::new(period, thread);
            let max_count = 4;
            let (ids, storage) = pool.get_block_operations(&target_slot);
            assert!(ids
                .iter()
                .map(|id| (*id, storage.retrieve_operation(id).unwrap().serialized_data))
                .eq(thread_tx_lists[target_slot.thread as usize]
                    .iter()
                    .filter(|(_, r)| r.contains(&target_slot.period))
                    .take(max_count)
                    .map(|(op, _)| (op.id, op.serialized_data.clone()))));
        }
    }

    // add transactions with a high fee but too much in the future: should be ignored
    {
        pool.update_current_slot(Slot::new(10, 0));
        let fee = 1000;
        let expire_period: u64 = 300;
        let op = get_transaction(expire_period, fee);
        let id = op.verify_integrity().unwrap();
        let mut storage = Storage::default();
        storage.store_operations(vec![op.clone()]);
        pool.add_operations(storage).unwrap();
        assert_eq!(storage.get_op_refs(), Set::<OperationId>::default());
        let res = pool.get_block_operations(&Slot::new(expire_period - 1, op.thread));
        assert!(res.is_empty());
    }
}
