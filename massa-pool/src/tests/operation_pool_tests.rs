use massa_models::{
    prehash::{Map, Set},
    signed::Signed,
    Address, Amount, Operation, OperationId, OperationType, SerializeCompact, Slot,
};
use massa_signature::{derive_public_key, generate_random_private_key};
use serial_test::serial;
use std::str::FromStr;

use crate::operation_pool::OperationPool;

use super::settings::POOL_CONFIG;

fn get_transaction(expire_period: u64, fee: u64) -> (Signed<Operation, OperationId>, u8) {
    let sender_priv = generate_random_private_key();
    let sender_pub = derive_public_key(&sender_priv);

    let recv_priv = generate_random_private_key();
    let recv_pub = derive_public_key(&recv_priv);

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_pub),
        amount: Amount::default(),
    };
    let content = Operation {
        fee: Amount::from_str(&fee.to_string()).unwrap(),
        op,
        sender_public_key: sender_pub,
        expire_period,
    };
    (
        Signed::new_signed(content, &sender_priv).unwrap().1,
        Address::from_public_key(&sender_pub).get_thread(2),
    )
}

#[test]
#[serial]
fn test_pool() {
    let mut pool = OperationPool::new(&POOL_CONFIG);

    // generate (id, transactions, range of validity) by threads
    let mut thread_tx_lists = vec![Vec::new(); POOL_CONFIG.thread_count as usize];
    for i in 0..18 {
        let fee = 40 + i;
        let expire_period: u64 = 40 + i;
        let start_period = expire_period.saturating_sub(POOL_CONFIG.operation_validity_periods);
        let (op, thread) = get_transaction(expire_period, fee);
        let id = op.verify_integrity().unwrap();

        let mut ops = Map::default();
        ops.insert(id, op.clone());

        let newly_added = pool.add_operations(ops.clone()).unwrap();
        assert_eq!(newly_added, ops.keys().copied().collect());

        // duplicate
        let newly_added = pool.add_operations(ops).unwrap();
        assert_eq!(newly_added, Set::<OperationId>::default());

        thread_tx_lists[thread as usize].push((id, op, start_period..=expire_period));
    }

    // sort from bigger fee to smaller and truncate
    for lst in thread_tx_lists.iter_mut() {
        lst.reverse();
        lst.truncate(POOL_CONFIG.settings.max_pool_size_per_thread as usize);
    }

    // checks ops are the expected ones for thread 0 and 1 and various periods
    for thread in 0u8..=1 {
        for period in 0u64..70 {
            let target_slot = Slot::new(period, thread);
            let max_count = 3;
            let res = pool
                .get_operation_batch(target_slot, Set::<OperationId>::default(), max_count, 10000)
                .unwrap();
            assert!(res
                .iter()
                .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))
                .eq(thread_tx_lists[target_slot.thread as usize]
                    .iter()
                    .filter(|(_, _, r)| r.contains(&target_slot.period))
                    .take(max_count)
                    .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))));
        }
    }

    // op ending before or at period 45 won't appear in the block due to incompatible validity range
    // we don't keep them as expected ops
    let final_period = 45u64;
    pool.update_latest_final_periods(vec![final_period; POOL_CONFIG.thread_count as usize])
        .unwrap();
    for lst in thread_tx_lists.iter_mut() {
        lst.retain(|(_, op, _)| op.content.expire_period > final_period);
    }

    // checks ops are the expected ones for thread 0 and 1 and various periods
    for thread in 0u8..=1 {
        for period in 0u64..70 {
            let target_slot = Slot::new(period, thread);
            let max_count = 4;
            let res = pool
                .get_operation_batch(target_slot, Set::<OperationId>::default(), max_count, 10000)
                .unwrap();
            assert!(res
                .iter()
                .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))
                .eq(thread_tx_lists[target_slot.thread as usize]
                    .iter()
                    .filter(|(_, _, r)| r.contains(&target_slot.period))
                    .take(max_count)
                    .map(|(id, op, _)| (id, op.to_bytes_compact().unwrap()))));
        }
    }

    // add transactions with a high fee but too much in the future: should be ignored
    {
        pool.update_current_slot(Slot::new(10, 0));
        let fee = 1000;
        let expire_period: u64 = 300;
        let (op, thread) = get_transaction(expire_period, fee);
        let id = op.verify_integrity().unwrap();
        let mut ops = Map::default();
        ops.insert(id, op);
        let newly_added = pool.add_operations(ops).unwrap();
        assert_eq!(newly_added, Set::<OperationId>::default());
        let res = pool
            .get_operation_batch(
                Slot::new(expire_period - 1, thread),
                Set::<OperationId>::default(),
                10,
                10000,
            )
            .unwrap();
        assert!(res.is_empty());
    }
}
