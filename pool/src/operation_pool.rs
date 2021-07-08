use std::{
    cmp::max,
    collections::{BTreeSet, HashMap, HashSet},
};

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

    fn is_valid_at_period(&self, period: u64, operation_validity_periods: u64) -> bool {
        let start = max(
            0,
            self.op.content.expire_period as i64 - operation_validity_periods as i64,
        ) as u64;
        Slot::new(period, self.thread) >= Slot::new(start, self.thread)
            && Slot::new(self.op.content.expire_period, self.thread)
                >= Slot::new(period, self.thread)
    }
}

pub struct OperationPool {
    ops: HashMap<OperationId, WrappedOperation>,
    /// one vec per thread
    ops_by_thread_and_interest:
        Vec<BTreeSet<(std::cmp::Reverse<num::rational::Ratio<u64>>, OperationId)>>, // [thread][order by: (rev rentability, OperationId)]
    /// latest final blocks periods
    last_final_periods: Vec<u64>,
    current_slot: Slot,
    cfg: PoolConfig,
}

impl OperationPool {
    pub fn new(cfg: PoolConfig) -> OperationPool {
        OperationPool {
            ops: HashMap::new(),
            ops_by_thread_and_interest: vec![BTreeSet::new(); cfg.thread_count as usize],
            current_slot: Slot::new(0, 0),
            last_final_periods: vec![0; cfg.thread_count as usize],
            cfg,
        }
    }

    pub fn contains(&self, op: OperationId) -> bool {
        self.ops.contains_key(&op)
    }

    /// Incomming operations
    ///
    /// Promote operation if already present.
    /// Else check validity and then insert
    /// * current_slot in validity period
    /// * signature ok
    /// Ask new operation for propagation
    ///
    /// An error is returned when a critically wrrong operation was received
    pub fn add_operations(
        &mut self,
        operations: Vec<(OperationId, Operation)>,
        context: &SerializationContext,
    ) -> Result<(), PoolError> {
        for (op_id, operation) in operations.into_iter() {
            // Already present
            if self.ops.contains_key(&op_id) {
                return Ok(());
            }

            let wrapped_op = WrappedOperation::new(operation, self.cfg.thread_count, context)?;
            let thread = wrapped_op.thread;
            let interest = (std::cmp::Reverse(wrapped_op.get_fee_density()), op_id);

            let start = max(
                0,
                wrapped_op.op.content.expire_period as i64
                    - self.cfg.operation_validity_periods as i64,
            ) as u64;
            // period validity check
            if self.current_slot < Slot::new(start, wrapped_op.thread)
                || Slot::new(wrapped_op.op.content.expire_period, wrapped_op.thread)
                    < Slot::new(
                        self.last_final_periods[wrapped_op.thread as usize],
                        wrapped_op.thread,
                    )
            {
                return Ok(());
            }

            self.ops_by_thread_and_interest[thread as usize].insert(interest);
            self.ops.insert(op_id, wrapped_op);
            // remove excess operation if pool is full
            while self.ops_by_thread_and_interest[thread as usize].len()
                > self.cfg.max_pool_size as usize
            {
                // should be one iteration
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
        self.current_slot = slot;
        self.prune();
    }

    fn prune(&mut self) {
        let ids = self
            .ops
            .iter()
            .filter(|(_id, op)| {
                Slot::new(op.op.content.expire_period, op.thread)
                    < Slot::new(
                        self.last_final_periods[op.thread as usize].clone(),
                        op.thread,
                    )
            })
            .map(|(id, _)| id.clone())
            .collect();

        self.remove_op(ids);
    }

    pub fn update_latest_final_periods(&mut self, periods: Vec<u64>) {
        self.last_final_periods = periods;
        self.prune()
    }

    // removes an operation
    fn remove_op(&mut self, op_ids: Vec<OperationId>) {
        for op_id in op_ids.into_iter() {
            if let Some(wrapped_op) = self.ops.remove(&op_id) {
                // complexity: const
                let interest = (std::cmp::Reverse(wrapped_op.get_fee_density()), op_id);
                self.ops_by_thread_and_interest[wrapped_op.thread as usize].remove(&interest);
                // complexity: log
            }
        }
    }

    // /// Update current_slot and discard invalid or integrated operation
    // pub fn ack_final_block(&mut self, periods: Vec<u64>) -> Result<(), ConsensusError> {
    //     for (thread, period) in periods.iter().enumerate() {
    //         if self.current_periods[thread] != *period {
    //             // if update is needed
    //             let to_remove: Vec<_> = self
    //                 .ops
    //                 .iter()
    //                 .filter(|(_id, op)| {
    //                     !op.is_valid_at_period(*period, self.cfg.operation_validity_periods)
    //                 })
    //                 .map(|(id, _op)| id.clone())
    //                 .collect();
    //             for id in to_remove.iter() {
    //                 self.remove_op(*id);
    //             }
    //         }
    //     }
    //     Ok(())
    // }

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
                    if !w_op.is_valid_at_period(block_slot.period, self.cfg.operation_validity_periods) {
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
