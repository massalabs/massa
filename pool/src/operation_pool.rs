use std::collections::{BTreeSet, HashMap, HashSet};

use models::{Address, Operation, OperationId, SerializationContext, SerializeCompact, Slot};
use num::rational::Ratio;

use crate::{PoolConfig, PoolError};

struct WrappedOperation {
    op: Operation,
    byte_count: u64,
    thread: u8,
}

impl WrappedOperation {
    fn new(
        op: Operation,
        thread_count: u8,
        context: &SerializationContext,
    ) -> Result<Self, PoolError> {
        Ok(WrappedOperation {
            byte_count: op.to_bytes_compact(&context)?.len() as u64,
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
}

impl OperationPool {
    pub fn new(cfg: PoolConfig, thread_count: u8) -> OperationPool {
        OperationPool {
            ops: HashMap::new(),
            ops_by_thread_and_interest: vec![BTreeSet::new(); thread_count as usize],
            current_slot: None,
            last_final_periods: vec![0; thread_count as usize],
            cfg,
            thread_count,
        }
    }

    pub fn contains(&self, op: OperationId) -> bool {
        self.ops.contains_key(&op)
    }

    /// Incomming operations
    ///
    pub fn add_operations(
        &mut self,
        operations: Vec<(OperationId, Operation)>,
        context: &SerializationContext,
    ) -> Result<(), PoolError> {
        for (op_id, operation) in operations.into_iter() {
            // Already present
            if self.ops.contains_key(&op_id) {
                continue;
            }

            // wrap
            let wrapped_op = WrappedOperation::new(operation, self.thread_count, context)?;

            // check if expired
            if wrapped_op.op.content.expire_period
                <= self.last_final_periods[wrapped_op.thread as usize]
            {
                continue;
            }

            // insert
            let interest = (std::cmp::Reverse(wrapped_op.get_fee_density()), op_id);
            self.ops_by_thread_and_interest[wrapped_op.thread as usize].insert(interest);
            self.ops.insert(op_id, wrapped_op);
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
            }
        }

        Ok(())
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
        max_count: usize,
    ) -> Result<Vec<(OperationId, Operation)>, PoolError> {
        self.ops_by_thread_and_interest[block_slot.thread as usize]
            .iter()
            .filter_map(|(_rentability, id)| {
                if exclude.contains(id) {
                    return None;
                }
                if let Some(w_op) = self.ops.get(id) {
                    if !(w_op.op.content.expire_period.saturating_sub(self.cfg.operation_validity_periods)..=w_op.op.content.expire_period)
                        .contains(&block_slot.period) {
                        return None;
                    }
                    Some(Ok((id.clone(), w_op.op.clone())))
                } else {
                    Some(Err(PoolError::ContainerInconsistency(
                        format!("operation pool get_ops inconsistency: op_id={:?} is in ops_by_thread_and_interest but not in ops", id)
                    )))
                }
            })
            .take(max_count)
            .collect()
    }
}
