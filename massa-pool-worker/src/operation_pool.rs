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

    /// last consensus final periods, per thread
    pub last_cs_final_periods: Vec<u64>,
}

impl OperationPool {
    pub fn init(config: PoolConfig, storage: Storage) -> Self {
        OperationPool {
            sorted_ops_per_thread: vec![Default::default(); config.thread_count as usize],
            ops_per_expiration: Default::default(),
            last_cs_final_periods: vec![0u64, config.thread_count as usize],
            config,
            storage,
        }
    }

    /// notify of new final slot
    pub fn notify_final_cs_periods(&mut self, final_cs_periods: &Vec<u64>) {
        // update internal final slot counter
        self.last_cs_final_periods = final_cs_periods.clone();

        // prune old ops
        let removed_ops = Vec::new();
        while let Some((expire_slot, key)) = self.ops_per_expiration.first().copied() {
            if expire_slot.period > self.last_cs_final_periods[expire_slot.thread as usize] {
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

    /// Checks if an operation is relevant according to its thread and period validity range
    fn is_operation_relevant(
        &self,
        thread: u8,
        period_validity_range: &RangeInclusive<u64>,
    ) -> bool {
        // too old
        if &period_validity_range.end() <= self.last_cs_final_periods[thread as usize] {
            return false;
        }

        // validity not started yet
        //TODO eliminate ops whose validity hasn't started yet

        true
    }

    /// Add a list of operations to the pool
    pub fn add_operations(&mut self, mut ops_storage: Storage) {
        let items = ops_storage.get_op_refs().clone();

        let mut added = Set::with_capacity(items.len());
        let mut removed = Set::with_capacity(items.len());

        // add items to pool
        ops_storage.with_operations(&items, |op_refs| {
            op_refs.iter().zip(items.iter()).for_each(|(op_ref, id)| {
                let op = op_ref
                    .expect("attempting to add operation to pool, but it is absent from storage");
                let op_validity = op.get_validity_range(self.config.operation_validity_period);
                if !self.is_operation_relevant(op.thread, &op_validity) {
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
                        added.insert(id);
                    }
                }
            });
        });

        // prune excess operations

        self.sorted_ops_per_thread.iter_mut().for_each(|ops| {
            while ops.len() > self.config.max_ops_pool_size_per_thread {
                // the unrap below won't panic because the loop condition tests for non-emptines of self.operations
                let (key, op_info) = ops.pop_last().unwrap();
                let end_slot = Slot::new(*op_info.validity_period_range.end(), op_info.thread);
                self.ops_per_expiration.remove(&(end_slot, key));
                if !added.remove(&op_info.id) {
                    removed.insert(op_info.id);
                }
            }
        });

        // take ownership on added ops
        self.storage
            .extend(ops_storage.split_off(Default::default(), added, Default::default()));

        // drop removed ops from storage
        self.storage.drop_operation_refs(&removed);
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
