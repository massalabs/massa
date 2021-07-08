use std::collections::{BTreeSet, HashMap, HashSet};

use models::{Address, Operation, OperationId, SerializeCompact, Slot};
use num::rational::Ratio;

use crate::{PoolConfig, PoolError};

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
        Ratio::new(self.op.content.fee, self.byte_count)
    }
}

pub struct OperationPool {
    ops: HashMap<OperationId, WrappedOperation>,
    /// one vec per thread
    ops_by_thread_and_interest:
        Vec<BTreeSet<(std::cmp::Reverse<num::rational::Ratio<u64>>, OperationId)>>, // [thread][order by: (rev rentability, OperationId)]
    /// latest final blocks periods
    last_final_periods: Vec<u64>,
    /// current slot
    current_slot: Option<Slot>,
    /// config
    cfg: PoolConfig,
    /// thread count
    thread_count: u8,
    /// operation validity periods
    operation_validity_periods: u64,
}

impl OperationPool {
    pub fn new(
        cfg: PoolConfig,
        thread_count: u8,
        operation_validity_periods: u64,
    ) -> OperationPool {
        OperationPool {
            ops: HashMap::new(),
            ops_by_thread_and_interest: vec![BTreeSet::new(); thread_count as usize],
            current_slot: None,
            last_final_periods: vec![0; thread_count as usize],
            cfg,
            thread_count,
            operation_validity_periods,
        }
    }

    /// Incomming operations. Returns newly added
    ///
    pub fn add_operations(
        &mut self,
        operations: HashMap<OperationId, Operation>,
    ) -> Result<HashSet<OperationId>, PoolError> {
        let mut newly_added = HashSet::new();

        for (op_id, operation) in operations.into_iter() {
            massa_trace!("pool add_operations op", { "op": operation });

            // Already present
            if self.ops.contains_key(&op_id) {
                massa_trace!("pool add_operations  op already present.)", {});
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
                    > self.cfg.max_operation_future_validity_start_periods
                {
                    massa_trace!("pool add_operations validity_start_period >  self.cfg.max_operation_future_validity_start_periods", {
                        "range": validity_start_period.saturating_sub(cur_period_in_thread),
                        "max_operation_future_validity_start_periods": self.cfg.max_operation_future_validity_start_periods
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
            self.ops_by_thread_and_interest[wrapped_op.thread as usize].insert(interest);
            self.ops.insert(op_id, wrapped_op);
            newly_added.insert(op_id);
        }

        // remove excess operations if pool is full
        for thread in 0..self.thread_count {
            while self.ops_by_thread_and_interest[thread as usize].len()
                > self.cfg.max_pool_size_per_thread as usize
            {
                let (_removed_rentability, removed_id) = self.ops_by_thread_and_interest
                    [thread as usize]
                    .pop_last()
                    .unwrap(); // will not panic because of the while condition. complexity = log or better
                self.ops.remove(&removed_id); // complexity: const
                newly_added.remove(&removed_id);
            }
        }

        Ok(newly_added)
    }

    pub fn update_current_slot(&mut self, slot: Slot) {
        self.current_slot = Some(slot);
        self.prune();
    }

    fn prune(&mut self) {
        let ids = self
            .ops
            .iter()
            .filter(|(_id, w_op)| {
                w_op.op.content.expire_period <= self.last_final_periods[w_op.thread as usize]
            })
            .map(|(id, _)| id.clone())
            .collect();

        self.remove_ops(ids);
    }

    pub fn update_latest_final_periods(&mut self, periods: Vec<u64>) {
        self.last_final_periods = periods;
        self.prune()
    }

    // removes an operation
    fn remove_ops(&mut self, op_ids: Vec<OperationId>) {
        for op_id in op_ids.into_iter() {
            if let Some(wrapped_op) = self.ops.remove(&op_id) {
                // complexity: const
                let interest = (std::cmp::Reverse(wrapped_op.get_fee_density()), op_id);
                self.ops_by_thread_and_interest[wrapped_op.thread as usize].remove(&interest);
                // complexity: log
            }
        }
    }

    /// Get max_count operation for thread block_slot.thread
    /// if vec is not full that means that there is no more interesting transactions left
    pub fn get_operation_batch(
        &mut self,
        block_slot: Slot,
        exclude: HashSet<OperationId>,
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
                    Some(Ok((id.clone(), w_op.op.clone(), w_op.byte_count)))
                } else {
                    Some(Err(PoolError::ContainerInconsistency(
                        format!("operation pool get_ops inconsistency: op_id={:?} is in ops_by_thread_and_interest but not in ops", id)
                    )))
                }
            })
            .take(batch_size)
            .collect()
    }

    pub fn get_operations(
        &self,
        operation_ids: &HashSet<OperationId>,
    ) -> HashMap<OperationId, Operation> {
        operation_ids
            .iter()
            .filter_map(|op_id| self.ops.get(&op_id).map(|w_op| (*op_id, w_op.op.clone())))
            .collect()
    }

    pub fn get_operations_involving_address(
        &self,
        address: &Address,
    ) -> Result<HashSet<OperationId>, PoolError> {
        let mut res = HashSet::new();
        for (id, op) in self.ops.iter() {
            let involved = op.op.get_involved_addresses(None)?;
            if involved.contains(address) {
                res.insert(*id);
            }
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto::hash::Hash;
    use models::{Operation, OperationContent, OperationType};
    use serial_test::serial;

    fn example_pool_config() -> (PoolConfig, u8, u64) {
        let mut nodes = Vec::new();
        for _ in 0..2 {
            let private_key = crypto::generate_random_private_key();
            let public_key = crypto::derive_public_key(&private_key);
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
            max_block_size: max_block_size,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_ask_blocks_per_message: 10,
            max_operations_per_message: 1024,
            max_bootstrap_message_size: 100000000,
        });

        (
            PoolConfig {
                max_pool_size_per_thread: 100000,
                max_operation_future_validity_start_periods: 200,
            },
            thread_count,
            operation_validity_periods,
        )
    }

    fn get_transaction(expire_period: u64, fee: u64) -> (Operation, u8) {
        let sender_priv = crypto::generate_random_private_key();
        let sender_pub = crypto::derive_public_key(&sender_priv);

        let recv_priv = crypto::generate_random_private_key();
        let recv_pub = crypto::derive_public_key(&recv_priv);

        let op = OperationType::Transaction {
            recipient_address: Address::from_public_key(&recv_pub).unwrap(),
            amount: 0,
        };
        let content = OperationContent {
            fee,
            op,
            sender_public_key: sender_pub,
            expire_period,
        };
        let hash = Hash::hash(&content.to_bytes_compact().unwrap());
        let signature = crypto::sign(&hash, &sender_priv).unwrap();

        (
            Operation { content, signature },
            Address::from_public_key(&sender_pub).unwrap().get_thread(2),
        )
    }

    #[test]
    #[serial]
    fn test_pool() {
        let (mut cfg, thread_count, operation_validity_periods) = example_pool_config();

        let max_pool_size_per_thread = 10;
        cfg.max_pool_size_per_thread = max_pool_size_per_thread;

        let mut pool = OperationPool::new(cfg, thread_count, operation_validity_periods);

        // generate transactions
        let mut thread_tx_lists = vec![Vec::new(); thread_count as usize];
        for i in 0..18 {
            let fee = 40 + i;
            let expire_period: u64 = 40 + i;
            let start_period = expire_period.saturating_sub(operation_validity_periods);
            let (op, thread) = get_transaction(expire_period, fee);
            let id = op.verify_integrity().unwrap();

            let mut ops = HashMap::new();
            ops.insert(id, op.clone());

            let newly_added = pool.add_operations(ops.clone()).unwrap();
            assert_eq!(newly_added, ops.keys().copied().collect());

            // duplicate
            let newly_added = pool.add_operations(ops).unwrap();
            assert_eq!(newly_added, HashSet::new());

            thread_tx_lists[thread as usize].push((id, op, start_period..=expire_period));
        }

        // sort from bigger fee to smaller and truncate
        for lst in thread_tx_lists.iter_mut() {
            lst.reverse();
            lst.truncate(max_pool_size_per_thread as usize);
        }

        // checks ops for thread 0 and 1 and various periods
        for thread in 0u8..=1 {
            for period in 0u64..70 {
                let target_slot = Slot::new(period, thread);
                let max_count = 3;
                let res = pool
                    .get_operation_batch(target_slot, HashSet::new(), max_count, 10000)
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
        pool.update_latest_final_periods(vec![final_period; thread_count as usize]);
        for lst in thread_tx_lists.iter_mut() {
            lst.retain(|(_, op, _)| op.content.expire_period > final_period);
        }

        // checks ops for thread 0 and 1 and various periods
        for thread in 0u8..=1 {
            for period in 0u64..70 {
                let target_slot = Slot::new(period, thread);
                let max_count = 4;
                let res = pool
                    .get_operation_batch(target_slot, HashSet::new(), max_count, 10000)
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
            let mut ops = HashMap::new();
            ops.insert(id, op);
            let newly_added = pool.add_operations(ops).unwrap();
            assert_eq!(newly_added, HashSet::new());
            let res = pool
                .get_operation_batch(
                    Slot::new(expire_period - 1, thread),
                    HashSet::new(),
                    10,
                    10000,
                )
                .unwrap();
            assert!(res.is_empty());
        }
    }
}
