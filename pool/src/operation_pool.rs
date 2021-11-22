// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{PoolError, PoolSettings};
use models::{
    address::AddressHashMap, Address, BlockHashMap, Operation, OperationHashMap, OperationHashSet,
    OperationId, OperationSearchResult, OperationSearchResultStatus, SerializeCompact, Slot,
};
use num::rational::Ratio;
use std::{collections::BTreeSet, usize};

struct OperationIndex(AddressHashMap<OperationHashSet>);

impl OperationIndex {
    fn new() -> OperationIndex {
        OperationIndex(AddressHashMap::default())
    }
    fn insert_op(&mut self, addr: Address, op_id: OperationId) {
        self.0
            .entry(addr)
            .or_insert_with(OperationHashSet::default)
            .insert(op_id);
    }

    fn get_ops_for_address(&self, address: &Address) -> Option<&OperationHashSet> {
        self.0.get(address)
    }

    fn remove_op_for_address(&mut self, address: &Address, op_id: &OperationId) {
        if let Some(old) = self.0.get_mut(address) {
            old.remove(op_id);
            if old.is_empty() {
                self.0.remove(address);
            }
        }
    }
}

struct WrappedOperation {
    op: Operation,
    byte_count: u64,
    thread: u8,
}

impl WrappedOperation {
    fn new(op: Operation, thread_count: u8) -> Result<Self, PoolError> {
        Ok(WrappedOperation {
            byte_count: op.to_bytes_compact()?.len() as u64,
            thread: Address::from_public_key(&op.content.sender_public_key)?
                .get_thread(thread_count),
            op,
        })
    }

    /// Used to compare operations
    fn get_fee_density(&self) -> Ratio<u64> {
        Ratio::new(self.op.content.fee.to_raw(), self.byte_count)
    }
}

pub struct OperationPool {
    ops: OperationHashMap<WrappedOperation>,
    /// one vec per thread
    ops_by_thread_and_interest:
        Vec<BTreeSet<(std::cmp::Reverse<num::rational::Ratio<u64>>, OperationId)>>, // [thread][order by: (rev rentability, OperationId)]
    /// Maps Addres -> Op id
    ops_by_address: OperationIndex,
    /// latest final blocks periods
    last_final_periods: Vec<u64>,
    /// current slot
    current_slot: Option<Slot>,
    /// config
    pool_settings: &'static PoolSettings,
    /// thread count
    thread_count: u8,
    /// operation validity periods
    operation_validity_periods: u64,
    /// ids of operations that are final with expire period and thread
    final_operations: OperationHashMap<(u64, u8)>,
}

impl OperationPool {
    pub fn new(
        pool_settings: &'static PoolSettings,
        thread_count: u8,
        operation_validity_periods: u64,
    ) -> OperationPool {
        OperationPool {
            ops: Default::default(),
            ops_by_thread_and_interest: vec![BTreeSet::new(); thread_count as usize],
            current_slot: None,
            last_final_periods: vec![0; thread_count as usize],
            pool_settings,
            thread_count,
            operation_validity_periods,
            final_operations: Default::default(),
            ops_by_address: OperationIndex::new(),
        }
    }

    /// Incoming operations. Returns newly added
    ///
    pub fn add_operations(
        &mut self,
        operations: OperationHashMap<Operation>,
    ) -> Result<OperationHashSet, PoolError> {
        let mut newly_added = OperationHashSet::default();

        for (op_id, operation) in operations.into_iter() {
            massa_trace!("pool add_operations op", { "op": operation });

            // Already present
            if self.ops.contains_key(&op_id) {
                massa_trace!("pool add_operations op already present", {});
                continue;
            }

            // already final
            if self.final_operations.contains_key(&op_id) {
                massa_trace!("pool add_operations op already final", {});
                continue;
            }

            // wrap
            let wrapped_op = WrappedOperation::new(operation, self.thread_count)?;

            // check if too much in the future
            if let Some(cur_slot) = self.current_slot {
                let cur_period_in_thread = if cur_slot.thread >= wrapped_op.thread {
                    cur_slot.period
                } else {
                    cur_slot.period.saturating_sub(1)
                };

                let validity_start_period = *wrapped_op
                    .op
                    .get_validity_range(self.operation_validity_periods)
                    .start();

                if validity_start_period.saturating_sub(cur_period_in_thread)
                    > self
                        .pool_settings
                        .max_operation_future_validity_start_periods
                {
                    massa_trace!("pool add_operations validity_start_period >  self.cfg.max_operation_future_validity_start_periods", {
                        "range": validity_start_period.saturating_sub(cur_period_in_thread),
                        "max_operation_future_validity_start_periods": self.pool_settings.max_operation_future_validity_start_periods
                    });
                    continue;
                }
            }

            // check if expired
            if wrapped_op.op.content.expire_period
                <= self.last_final_periods[wrapped_op.thread as usize]
            {
                massa_trace!("pool add_operations wrapped_op.op.content.expire_period <= self.last_final_periods[wrapped_op.thread as usize]", {
                    "expire_period": wrapped_op.op.content.expire_period,
                    "self.last_final_periods[wrapped_op.thread as usize]": self.last_final_periods[wrapped_op.thread as usize]
                });
                continue;
            }

            // insert
            let interest = (std::cmp::Reverse(wrapped_op.get_fee_density()), op_id);
            let addrs = wrapped_op.op.get_ledger_involved_addresses()?;

            self.ops_by_thread_and_interest[wrapped_op.thread as usize].insert(interest);
            self.ops.insert(op_id, wrapped_op);

            addrs.iter().for_each(|addr| {
                self.ops_by_address.insert_op(*addr, op_id);
            });

            newly_added.insert(op_id);
        }

        // remove excess operations if pool is full
        for thread in 0..self.thread_count {
            while self.ops_by_thread_and_interest[thread as usize].len()
                > self.pool_settings.max_pool_size_per_thread as usize
            {
                let (_removed_rentability, removed_id) = self.ops_by_thread_and_interest
                    [thread as usize]
                    .pop_last()
                    .unwrap(); // will not panic because of the while condition. complexity = log or better
                if let Some(removed_op) = self.ops.remove(&removed_id) {
                    // complexity: const
                    let addrs = removed_op.op.get_ledger_involved_addresses()?;
                    for addr in addrs {
                        self.ops_by_address
                            .remove_op_for_address(&addr, &removed_id);
                    }
                }
                newly_added.remove(&removed_id);
            }
        }

        Ok(newly_added)
    }

    pub fn new_final_operations(
        &mut self,
        ops: OperationHashMap<(u64, u8)>,
    ) -> Result<(), PoolError> {
        for (id, _) in ops.iter() {
            if let Some(wrapped) = self.ops.remove(id) {
                self.ops_by_thread_and_interest[wrapped.thread as usize]
                    .remove(&(std::cmp::Reverse(wrapped.get_fee_density()), *id));
                let addrs = wrapped.op.get_ledger_involved_addresses()?;
                for addr in addrs {
                    self.ops_by_address.remove_op_for_address(&addr, id);
                }
            } // else final op wasn't in pool.
        }
        self.final_operations.extend(ops);
        Ok(())
    }

    pub fn update_current_slot(&mut self, slot: Slot) {
        self.current_slot = Some(slot);
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    fn prune(&mut self) -> Result<(), PoolError> {
        let ids = self
            .ops
            .iter()
            .filter(|(_id, w_op)| {
                w_op.op.content.expire_period <= self.last_final_periods[w_op.thread as usize]
            })
            .map(|(id, _)| *id)
            .collect();

        self.remove_ops(ids)?;

        let ids = self
            .final_operations
            .iter()
            .filter(|(_, (exp, thread))| *exp <= self.last_final_periods[*thread as usize])
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();

        for id in ids.into_iter() {
            self.final_operations.remove(&id);
        }

        Ok(())
    }

    pub fn update_latest_final_periods(&mut self, periods: Vec<u64>) -> Result<(), PoolError> {
        self.last_final_periods = periods;
        self.prune()
    }

    // removes an operation
    fn remove_ops(&mut self, op_ids: Vec<OperationId>) -> Result<(), PoolError> {
        for op_id in op_ids.into_iter() {
            if let Some(wrapped_op) = self.ops.remove(&op_id) {
                // complexity: const
                let interest = (std::cmp::Reverse(wrapped_op.get_fee_density()), op_id);
                self.ops_by_thread_and_interest[wrapped_op.thread as usize].remove(&interest);
                // complexity: log

                let addrs = wrapped_op.op.get_ledger_involved_addresses()?;
                for addr in addrs {
                    self.ops_by_address.remove_op_for_address(&addr, &op_id);
                }
            }
        }
        Ok(())
    }

    /// Get max_count operation for thread block_slot.thread
    /// if vec is not full that means that there is no more interesting transactions left
    pub fn get_operation_batch(
        &mut self,
        block_slot: Slot,
        exclude: OperationHashSet,
        batch_size: usize,
        max_size: u64,
    ) -> Result<Vec<(OperationId, Operation, u64)>, PoolError> {
        self.ops_by_thread_and_interest[block_slot.thread as usize]
            .iter()
            .filter_map(|(_rentability, id)| {
                if exclude.contains(id) {
                    return None;
                }
                if let Some(w_op) = self.ops.get(id) {
                    if !w_op.op.get_validity_range(self.operation_validity_periods)
                        .contains(&block_slot.period) || w_op.byte_count > max_size {
                            massa_trace!("pool get_operation_batch not added to batch w_op.op.get_validity_range incorrect not added", {
                                "range": w_op.op.get_validity_range(self.operation_validity_periods),
                                "block_slot.period": block_slot.period
                            });
                        return None;
                    }
                    Some(Ok((*id, w_op.op.clone(), w_op.byte_count)))
                } else {
                    Some(Err(PoolError::ContainerInconsistency(
                        format!("operation pool get_ops inconsistency: op_id={} is in ops_by_thread_and_interest but not in ops", id)
                    )))
                }
            })
            .take(batch_size)
            .collect()
    }

    pub fn get_operations(&self, operation_ids: &OperationHashSet) -> OperationHashMap<Operation> {
        operation_ids
            .iter()
            .filter_map(|op_id| self.ops.get(op_id).map(|w_op| (*op_id, w_op.op.clone())))
            .collect()
    }

    pub fn get_operations_involving_address(
        &self,
        address: &Address,
    ) -> Result<OperationHashMap<OperationSearchResult>, PoolError> {
        if let Some(ids) = self.ops_by_address.get_ops_for_address(address) {
            ids.iter()
                .take(self.pool_settings.max_item_return_count)
                .map(|op_id| {
                    self.ops
                        .get(op_id)
                        .ok_or(PoolError::ContainerInconsistency(
                            "op in ops by address is not in ops".to_string(),
                        ))
                        .map(|op| {
                            (
                                *op_id,
                                OperationSearchResult {
                                    op: op.op.clone(),
                                    in_pool: true,
                                    in_blocks: BlockHashMap::default(),
                                    status: OperationSearchResultStatus::Pending,
                                },
                            )
                        })
                })
                .collect()
        } else {
            Ok(OperationHashMap::default())
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crypto::hash::Hash;
    use models::{Amount, Operation, OperationContent, OperationType};
    use serial_test::serial;
    use signature::{derive_public_key, generate_random_private_key, sign};
    use std::str::FromStr;

    lazy_static::lazy_static! {
        pub static ref POOL_SETTINGS: (PoolSettings, u8, u64, u64) = {
            let (mut cfg, thread_count, operation_validity_periods) = example_pool_config();

            let max_pool_size_per_thread = 10;
            cfg.max_pool_size_per_thread = max_pool_size_per_thread;

            (cfg, thread_count, operation_validity_periods, max_pool_size_per_thread)
        };
    }

    fn example_pool_config() -> (PoolSettings, u8, u64) {
        let mut nodes = Vec::new();
        for _ in 0..2 {
            let private_key = generate_random_private_key();
            let public_key = derive_public_key(&private_key);
            nodes.push((public_key, private_key));
        }
        let thread_count: u8 = 2;
        let operation_validity_periods: u64 = 50;
        let max_block_size = 1024 * 1024;
        let max_operations_per_block = 1024;

        // Init the serialization context with a default,
        // can be overwritten with a more specific one in the test.
        models::init_serialization_context(models::SerializationContext {
            max_block_operations: max_operations_per_block,
            parent_count: thread_count,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_block_size,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_ask_blocks_per_message: 10,
            max_operations_per_message: 1024,
            max_endorsements_per_message: 1024,
            max_bootstrap_message_size: 100000000,
            max_bootstrap_pos_entries: 1000,
            max_bootstrap_pos_cycles: 5,
            max_block_endorsments: 8,
        });

        (
            PoolSettings {
                max_pool_size_per_thread: 100000,
                max_operation_future_validity_start_periods: 200,
                max_endorsement_count: 1000,
                max_item_return_count: 1000,
            },
            thread_count,
            operation_validity_periods,
        )
    }

    fn get_transaction(expire_period: u64, fee: u64) -> (Operation, u8) {
        let sender_priv = generate_random_private_key();
        let sender_pub = derive_public_key(&sender_priv);

        let recv_priv = generate_random_private_key();
        let recv_pub = derive_public_key(&recv_priv);

        let op = OperationType::Transaction {
            recipient_address: Address::from_public_key(&recv_pub).unwrap(),
            amount: Amount::default(),
        };
        let content = OperationContent {
            fee: Amount::from_str(&fee.to_string()).unwrap(),
            op,
            sender_public_key: sender_pub,
            expire_period,
        };
        let hash = Hash::hash(&content.to_bytes_compact().unwrap());
        let signature = sign(&hash, &sender_priv).unwrap();

        (
            Operation { content, signature },
            Address::from_public_key(&sender_pub).unwrap().get_thread(2),
        )
    }

    #[test]
    #[serial]
    fn test_pool() {
        let (pool_settings, thread_count, operation_validity_periods, max_pool_size_per_thread): &(
            PoolSettings,
            u8,
            u64,
            u64,
        ) = &POOL_SETTINGS;

        let mut pool =
            OperationPool::new(pool_settings, *thread_count, *operation_validity_periods);

        // generate transactions
        let mut thread_tx_lists = vec![Vec::new(); *thread_count as usize];
        for i in 0..18 {
            let fee = 40 + i;
            let expire_period: u64 = 40 + i;
            let start_period = expire_period.saturating_sub(*operation_validity_periods);
            let (op, thread) = get_transaction(expire_period, fee);
            let id = op.verify_integrity().unwrap();

            let mut ops = OperationHashMap::default();
            ops.insert(id, op.clone());

            let newly_added = pool.add_operations(ops.clone()).unwrap();
            assert_eq!(newly_added, ops.keys().copied().collect());

            // duplicate
            let newly_added = pool.add_operations(ops).unwrap();
            assert_eq!(newly_added, OperationHashSet::default());

            thread_tx_lists[thread as usize].push((id, op, start_period..=expire_period));
        }

        // sort from bigger fee to smaller and truncate
        for lst in thread_tx_lists.iter_mut() {
            lst.reverse();
            lst.truncate(*max_pool_size_per_thread as usize);
        }

        // checks ops for thread 0 and 1 and various periods
        for thread in 0u8..=1 {
            for period in 0u64..70 {
                let target_slot = Slot::new(period, thread);
                let max_count = 3;
                let res = pool
                    .get_operation_batch(target_slot, OperationHashSet::default(), max_count, 10000)
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

        // op ending before or at period 45 should be discarded
        let final_period = 45u64;
        pool.update_latest_final_periods(vec![final_period; *thread_count as usize])
            .unwrap();
        for lst in thread_tx_lists.iter_mut() {
            lst.retain(|(_, op, _)| op.content.expire_period > final_period);
        }

        // checks ops for thread 0 and 1 and various periods
        for thread in 0u8..=1 {
            for period in 0u64..70 {
                let target_slot = Slot::new(period, thread);
                let max_count = 4;
                let res = pool
                    .get_operation_batch(target_slot, OperationHashSet::default(), max_count, 10000)
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
            let mut ops = OperationHashMap::default();
            ops.insert(id, op);
            let newly_added = pool.add_operations(ops).unwrap();
            assert_eq!(newly_added, OperationHashSet::default());
            let res = pool
                .get_operation_batch(
                    Slot::new(expire_period - 1, thread),
                    OperationHashSet::default(),
                    10,
                    10000,
                )
                .unwrap();
            assert!(res.is_empty());
        }
    }
}
