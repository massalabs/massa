// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{settings::PoolConfig, PoolError};
use massa_models::prehash::{Map, Set};
use massa_models::signed::Signed;
use massa_models::{
    Address, Operation, OperationId, OperationSearchResult, OperationSearchResultStatus,
    OperationType, SerializeCompact, Slot,
};
use num::rational::Ratio;
use std::{collections::BTreeSet, usize};

struct OperationIndex(Map<Address, Set<OperationId>>);

impl OperationIndex {
    fn new() -> OperationIndex {
        OperationIndex(Map::default())
    }
    fn insert_op(&mut self, addr: Address, op_id: OperationId) {
        self.0
            .entry(addr)
            .or_insert_with(Set::<OperationId>::default)
            .insert(op_id);
    }

    fn get_ops_for_address(&self, address: &Address) -> Option<&Set<OperationId>> {
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
    op: Signed<Operation, OperationId>,
    byte_count: u64,
    thread: u8,
}

impl WrappedOperation {
    fn new(op: Signed<Operation, OperationId>, thread_count: u8) -> Result<Self, PoolError> {
        Ok(WrappedOperation {
            byte_count: op.to_bytes_compact()?.len() as u64,
            thread: Address::from_public_key(&op.content.sender_public_key)
                .get_thread(thread_count),
            op,
        })
    }

    /// Gets the priority of the operation based on how much it profits the block producer
    /// vs how much space it takes in the block
    fn get_fee_density(&self) -> Ratio<u64> {
        // add inclusion fee
        let mut total_return = self.op.content.fee;

        // add gas fees
        if let OperationType::ExecuteSC {
            max_gas, gas_price, ..
        } = self.op.content.op
        {
            total_return = total_return.saturating_add(gas_price.saturating_mul_u64(max_gas));
        }

        // return ratio with size
        Ratio::new(total_return.to_raw(), self.byte_count)
    }
}

pub struct OperationPool {
    ops: Map<OperationId, WrappedOperation>,
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
    cfg: &'static PoolConfig,
    /// ids of operations that are final with expire period and thread
    final_operations: Map<OperationId, (u64, u8)>,
}

impl OperationPool {
    pub fn new(cfg: &'static PoolConfig) -> OperationPool {
        OperationPool {
            ops: Default::default(),
            ops_by_thread_and_interest: vec![BTreeSet::new(); cfg.thread_count as usize],
            current_slot: None,
            last_final_periods: vec![0; cfg.thread_count as usize],
            cfg,
            final_operations: Default::default(),
            ops_by_address: OperationIndex::new(),
        }
    }

    /// Incoming operations. Returns newly added
    ///
    pub fn add_operations(
        &mut self,
        operations: Map<OperationId, Signed<Operation, OperationId>>,
    ) -> Result<Set<OperationId>, PoolError> {
        let mut newly_added = Set::<OperationId>::default();

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
            let wrapped_op = WrappedOperation::new(operation, self.cfg.thread_count)?;

            // check if too much in the future
            if let Some(cur_slot) = self.current_slot {
                let cur_period_in_thread = if cur_slot.thread >= wrapped_op.thread {
                    cur_slot.period
                } else {
                    cur_slot.period.saturating_sub(1)
                };

                let validity_start_period = *wrapped_op
                    .op
                    .content
                    .get_validity_range(self.cfg.operation_validity_periods)
                    .start();

                if validity_start_period.saturating_sub(cur_period_in_thread)
                    > self
                        .cfg
                        .settings
                        .max_operation_future_validity_start_periods
                {
                    massa_trace!("pool add_operations validity_start_period >  self.cfg.max_operation_future_validity_start_periods", {
                        "range": validity_start_period.saturating_sub(cur_period_in_thread),
                        "max_operation_future_validity_start_periods": self.cfg.settings.max_operation_future_validity_start_periods
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
            let addrs = wrapped_op.op.content.get_ledger_involved_addresses()?;

            self.ops_by_thread_and_interest[wrapped_op.thread as usize].insert(interest);
            self.ops.insert(op_id, wrapped_op);

            addrs.iter().for_each(|addr| {
                self.ops_by_address.insert_op(*addr, op_id);
            });

            newly_added.insert(op_id);
        }

        // remove excess operations if pool is full
        for thread in 0..self.cfg.thread_count {
            while self.ops_by_thread_and_interest[thread as usize].len()
                > self.cfg.settings.max_pool_size_per_thread as usize
            {
                let (_removed_rentability, removed_id) = self.ops_by_thread_and_interest
                    [thread as usize]
                    .pop_last()
                    .unwrap(); // will not panic because of the while condition. complexity = log or better
                if let Some(removed_op) = self.ops.remove(&removed_id) {
                    // complexity: const
                    let addrs = removed_op.op.content.get_ledger_involved_addresses()?;
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
        ops: Map<OperationId, (u64, u8)>,
    ) -> Result<(), PoolError> {
        for (id, _) in ops.iter() {
            if let Some(wrapped) = self.ops.remove(id) {
                self.ops_by_thread_and_interest[wrapped.thread as usize]
                    .remove(&(std::cmp::Reverse(wrapped.get_fee_density()), *id));
                let addrs = wrapped.op.content.get_ledger_involved_addresses()?;
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
            .map(|(id, _)| *id);

        for id in ids.collect::<Vec<_>>() {
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

                let addrs = wrapped_op.op.content.get_ledger_involved_addresses()?;
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
        exclude: Set<OperationId>,
        batch_size: usize,
        max_size: u64,
    ) -> Result<Vec<(OperationId, Signed<Operation, OperationId>, u64)>, PoolError> {
        self.ops_by_thread_and_interest[block_slot.thread as usize]
            .iter()
            .filter_map(|(_rentability, id)| {
                if exclude.contains(id) {
                    return None;
                }
                if let Some(w_op) = self.ops.get(id) {
                    if !w_op.op.content.get_validity_range(self.cfg.operation_validity_periods)
                        .contains(&block_slot.period) || w_op.byte_count > max_size {
                            massa_trace!("pool get_operation_batch not added to batch w_op.op.content.get_validity_range incorrect not added", {
                                "range": w_op.op.content.get_validity_range(self.cfg.operation_validity_periods),
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

    pub fn get_operations(
        &self,
        operation_ids: &Set<OperationId>,
    ) -> Map<OperationId, Signed<Operation, OperationId>> {
        operation_ids
            .iter()
            .filter_map(|op_id| self.ops.get(op_id).map(|w_op| (*op_id, w_op.op.clone())))
            .collect()
    }

    pub fn get_operations_involving_address(
        &self,
        address: &Address,
    ) -> Result<Map<OperationId, OperationSearchResult>, PoolError> {
        if let Some(ids) = self.ops_by_address.get_ops_for_address(address) {
            ids.iter()
                .take(self.cfg.settings.max_item_return_count)
                .map(|op_id| {
                    self.ops
                        .get(op_id)
                        .ok_or_else(|| {
                            PoolError::ContainerInconsistency(
                                "op in ops by address is not in ops".to_string(),
                            )
                        })
                        .map(|op| {
                            (
                                *op_id,
                                OperationSearchResult {
                                    op: op.op.clone(),
                                    in_pool: true,
                                    in_blocks: Map::default(),
                                    status: OperationSearchResultStatus::Pending,
                                },
                            )
                        })
                })
                .collect()
        } else {
            Ok(Map::default())
        }
    }
}
