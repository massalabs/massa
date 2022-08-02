//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    prehash::{Map, Set},
    timeslots::get_current_latest_block_slot,
    Address, Amount, OperationId, Slot,
};
use massa_pool_exports::{PoolConfig, PoolOperationCursor};
use massa_storage::Storage;
use std::{
    collections::{btree_map, hash_map, BTreeMap, BTreeSet},
    ops::RangeInclusive,
};
use tracing::warn;

use crate::types::{build_cursor, OperationInfo};

pub struct OperationPool {
    /// config
    pub config: PoolConfig,

    /// operations sorted by decreasing quality, per thread
    pub sorted_ops_per_thread: Vec<BTreeMap<PoolOperationCursor, OperationInfo>>,

    /// operations sorted by increasing expiration slot
    pub ops_per_expiration: BTreeSet<(Slot, PoolOperationCursor)>,

    /// storage
    pub storage: Storage,

    /// last final slot
    pub last_final_slot: Slot,
}

impl OperationPool {
    pub fn init(config: PoolConfig, storage: Storage) -> Self {
        OperationPool {
            last_final_slot: Slot::new(0, config.thread_count.saturating_sub(1)),
            sorted_ops_per_thread: vec![Default::default(); config.thread_count as usize],
            ops_per_expiration: Default::default(),
            config,
            storage,
        }
    }

    /// notify of new final slot
    pub fn notify_final_slot(&mut self, slot: &Slot) {
        // update internal final slot counter
        self.last_final_slot = *slot;

        // prune old ops
        let removed_ops = Vec::new();
        while let Some((expire_slot, key)) = self.ops_per_expiration.first().copied() {
            if expire_slot > self.last_final_slot {
                break;
            }
            self.ops_per_expiration.pop_first();
            let info = self.sorted_ops_per_thread[expire_slot.thread as usize]
                .remove(&key)
                .expect("expected op presence in sorted list");
            removed_ops.push(info.id);
        }

        // notify storage that pool has lost references to removed_ops
        self.storage.drop_operation_refs(&removed_ops);
    }

    /// Checks if an operation is relevant according to its period validity range
    fn is_operation_relevant(&self, period_validity_range: &RangeInclusive<u64>) -> bool {
        &self.last_final_slot.period <= period_validity_range.end()
    }

    /// Add a list of operations to the pool
    pub fn add_operations(&mut self, mut ops_storage: Storage) {
        // add operations to pool
        let ops = ops_storage
            .get_op_refs()
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let mut added_ops = Set::with_capacity(ops.len());
        ops_storage.with_operations(&ops, |op_refs| {
            op_refs.iter().zip(ops.iter()).for_each(|(op_ref, id)| {
                let op = op_ref
                    .expect("attempting to add operation to pool, but it is absent from storage");
                let op_validity = op.get_validity_range(self.config.operation_validity_period);
                if !self.is_operation_relevant(&op_validity) {
                    return;
                }
                let key = build_cursor(op);
                // thread index won't panic because it was checked at op production or deserialization
                match self.sorted_ops_per_thread[op.thread as usize].entry(key) {
                    btree_map::Entry::Occupied(occ) => {}
                    btree_map::Entry::Vacant(vac) => {
                        self.ops_per_expiration
                            .insert((Slot::new(*op_validity.end(), op.thread), key));
                        vac.insert(OperationInfo::from_op(
                            op,
                            self.config.operation_validity_periods,
                        ));
                        added_ops.insert(id);
                    }
                }
            });
        });

        // prune excess operations
        let removed_ops = Set::with_capacity(ops.len());
        self.sorted_ops_per_thread.iter_mut().for_each(|ops| {
            while ops.len() > self.config.max_ops_pool_size_per_thread {
                // the unrap below won't panic because the loop condition tests for non-emptines of self.operations
                let (key, op_info) = ops.pop_last().unwrap();
                let end_slot = Slot::new(*op_info.validity_period_range.end(), op_info.thread);
                self.ops_per_expiration.remove(&(end_slot, key));
                if !added_ops.remove(&op_info.id) {
                    removed_ops.insert(op_info.id);
                }
            }
        });

        // take ownership on added ops
        self.storage
            .extend(ops_storage.split_off(Default::default(), added_ops));

        // drop removed ops from storage
        self.storage.drop_operation_refs(&removed_ops);
    }

    /// get operations for block creation
    pub fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage) {
        // init list of selected operation IDs
        let mut op_ids = Vec::new();

        // init remaining space
        let mut remaining_space = self.config.max_block_size;
        // init remaining gas
        let mut remaining_gas = self.config.max_block_gas;
        // cache of sequential balances
        let mut sequential_balance_cache: Map<Address, Amount> = Default::default();
        // list of previously excluded operation IDs
        let executed_ops = self.execution.get_executed_ops();

        // iterate over pool operations in the right thread, from best to worst
        for (_cursor, op_info) in self.sorted_ops_per_thread[slot.thread as usize].iter() {
            // exclude ops for which the block slot is outside of their validity range
            if !op_info.validity_period_range.contains(&slot.period) {
                continue;
            }

            // exclude ops that are too large
            if op_info.size > remaining_space {
                continue;
            }

            // exclude ops that require too much gas
            if op_info.max_gas > remaining_gas {
                continue;
            }

            // exclude ops that have been executed previously
            if executed_ops.contains(&op_info.id) {
                continue;
            }

            // check sequential balance
            let mut creator_seq_balance = sequential_balance_cache
                .entry(op_info.creator_address)
                .or_insert_with(|| {
                    self.execution
                        .get_sequential_balance(&op_info.creator_address)
                        .unwrap_or_default()
                });
            if &creator_seq_balance < op_info.max_sequential_spending {
                continue;
            }

            // here we consider the operation as accepted
            op_ids.push(op_info.id);

            // update remaining block space
            remaining_space -= op_info.size;

            // update remaining block gas
            remaining_gas -= op_info.max_gas;

            // update sequential balance cache
            *creator_seq_balance =
                creator_seq_balance.saturating_sub(op_info.max_sequential_spending);
        }

        // generate storage
        let mut res_storage = self.storage.clone_without_refs();
        res_storage.claim_operation_refs(&self.storage, &op_ids.iter().copied().collect());

        (op_ids, res_storage)
    }
}
