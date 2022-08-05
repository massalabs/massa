//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_execution_exports::ExecutionController;
use massa_models::{
    prehash::{BuildMap, Map, Set},
    Address, Amount, OperationId, Slot,
};
use massa_pool_exports::{PoolConfig, PoolOperationCursor};
use massa_storage::Storage;
use std::{
    collections::{btree_map, BTreeMap, BTreeSet},
    ops::RangeInclusive,
};

use crate::types::{build_cursor, OperationInfo};

pub struct OperationPool {
    /// config
    config: PoolConfig,

    /// operations sorted by decreasing quality, per thread
    sorted_ops_per_thread: Vec<BTreeMap<PoolOperationCursor, OperationInfo>>,

    /// operations sorted by increasing expiration slot
    ops_per_expiration: BTreeSet<(Slot, PoolOperationCursor)>,

    /// storage instance
    storage: Storage,

    /// execution controller
    execution_controller: Box<dyn ExecutionController>,

    /// last consensus final periods, per thread
    last_cs_final_periods: Vec<u64>,
}

impl OperationPool {
    pub fn init(
        config: PoolConfig,
        storage: Storage,
        execution_controller: Box<dyn ExecutionController>,
    ) -> Self {
        OperationPool {
            sorted_ops_per_thread: vec![Default::default(); config.thread_count as usize],
            ops_per_expiration: Default::default(),
            last_cs_final_periods: vec![0u64; config.thread_count as usize],
            config,
            storage,
            execution_controller,
        }
    }

    /// notify of new final slot
    pub fn notify_final_cs_periods(&mut self, final_cs_periods: &Vec<u64>) {
        // update internal final slot counter
        self.last_cs_final_periods = final_cs_periods.clone();

        // prune old ops
        let mut removed_ops: Set<_> = Default::default();
        while let Some((expire_slot, key)) = self.ops_per_expiration.first().copied() {
            if expire_slot.period > self.last_cs_final_periods[expire_slot.thread as usize] {
                break;
            }
            self.ops_per_expiration.pop_first();
            let info = self.sorted_ops_per_thread[expire_slot.thread as usize]
                .remove(&key)
                .expect("expected op presence in sorted list");
            removed_ops.insert(info.id);
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
        if *period_validity_range.end() <= self.last_cs_final_periods[thread as usize] {
            return false;
        }

        // validity not started yet
        //TODO eliminate ops whose validity hasn't started yet

        true
    }

    /// Add a list of operations to the pool
    pub fn add_operations(&mut self, mut ops_storage: Storage) {
        let items = ops_storage
            .get_op_refs()
            .iter()
            .copied()
            .collect::<Vec<_>>();

        let mut added = Set::with_capacity_and_hasher(items.len(), BuildMap::default());
        let mut removed = Set::with_capacity_and_hasher(items.len(), BuildMap::default());

        // add items to pool
        ops_storage.with_operations(&items, |op_refs| {
            op_refs.iter().zip(items.iter()).for_each(|(op_ref, id)| {
                let op = op_ref
                    .expect("attempting to add operation to pool, but it is absent from storage");
                let op_validity = op.get_validity_range(self.config.operation_validity_periods);
                if !self.is_operation_relevant(op.thread, &op_validity) {
                    return;
                }
                let key = build_cursor(op);
                // thread index won't panic because it was checked at op production or deserialization
                match self.sorted_ops_per_thread[op.thread as usize].entry(key) {
                    btree_map::Entry::Occupied(_) => {}
                    btree_map::Entry::Vacant(vac) => {
                        self.ops_per_expiration
                            .insert((Slot::new(*op_validity.end(), op.thread), key));
                        vac.insert(OperationInfo::from_op(
                            op,
                            self.config.operation_validity_periods,
                            self.config.roll_price,
                        ));
                        added.insert(*id);
                    }
                }
            });
        });

        // prune excess operations

        self.sorted_ops_per_thread.iter_mut().for_each(|ops| {
            while ops.len() > self.config.max_operation_pool_size_per_thread {
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
        self.storage.extend(ops_storage.split_off(
            &Default::default(),
            &added,
            &Default::default(),
        ));

        // drop removed ops from storage
        self.storage.drop_operation_refs(&removed);
    }

    /// get operations for block creation
    pub fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage) {
        // init list of selected operation IDs
        let mut op_ids = Vec::new();

        // init remaining space
        let mut remaining_space = self.config.max_block_size as usize;
        // init remaining gas
        let mut remaining_gas = self.config.max_block_gas;
        // cache of sequential balances
        let mut sequential_balance_cache: Map<Address, Amount> = Default::default();

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

            // check if the op was already executed
            // TOOD batch this
            if self
                .execution_controller
                .unexecuted_ops_among(&vec![op_info.id].into_iter().collect())
                .is_empty()
            {
                continue;
            }

            // check sequential balance
            let creator_seq_balance = sequential_balance_cache
                .entry(op_info.creator_address)
                .or_insert_with(|| {
                    self.execution_controller
                        .get_final_and_active_sequential_balance(vec![op_info.creator_address])[0]
                        .1
                        .unwrap_or_default()
                });
            if *creator_seq_balance < op_info.max_sequential_spending {
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
        let claim_ops: Set<OperationId> = op_ids.iter().copied().collect();
        let claimed_ops = res_storage.claim_operation_refs(&claim_ops);
        if claimed_ops.len() != claim_ops.len() {
            panic!("could not claim all operations from storage");
        }

        (op_ids, res_storage)
    }
}
