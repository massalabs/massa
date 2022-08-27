//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_execution_exports::ExecutionController;
use massa_models::{
    prehash::{BuildHashMapper, PreHashMap, PreHashSet},
    Address, Amount, OperationId, Slot,
};
use massa_pool_exports::PoolConfig;
use massa_storage::Storage;
use std::collections::BTreeSet;

use crate::types::{OperationInfo, PoolOperationCursor};

pub struct OperationPool {
    /// config
    config: PoolConfig,

    /// operations map
    operations: PreHashMap<OperationId, OperationInfo>,

    /// operations sorted by decreasing quality, per thread
    sorted_ops_per_thread: Vec<BTreeSet<PoolOperationCursor>>,

    /// operations sorted by increasing expiration slot
    ops_per_expiration: BTreeSet<(Slot, OperationId)>,

    /// storage instance
    pub(crate) storage: Storage,

    /// execution controller
    execution_controller: Box<dyn ExecutionController>,

    /// last consensus final periods, per thread
    last_cs_final_periods: Vec<u64>,
}

impl OperationPool {
    pub fn init(
        config: PoolConfig,
        storage: &Storage,
        execution_controller: Box<dyn ExecutionController>,
    ) -> Self {
        OperationPool {
            operations: Default::default(),
            sorted_ops_per_thread: vec![Default::default(); config.thread_count as usize],
            ops_per_expiration: Default::default(),
            last_cs_final_periods: vec![0u64; config.thread_count as usize],
            config,
            storage: storage.clone_without_refs(),
            execution_controller,
        }
    }

    /// Get the number of stored elements
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Checks whether an element is stored in the pool.
    pub fn contains(&self, id: &OperationId) -> bool {
        self.operations.contains_key(id)
    }

    /// notify of new final slot
    pub fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        // update internal final slot counter
        self.last_cs_final_periods = final_cs_periods.to_vec();

        // prune old ops
        let mut removed_ops: PreHashSet<_> = Default::default();
        while let Some((expire_slot, op_id)) = self.ops_per_expiration.first().copied() {
            if expire_slot.period > self.last_cs_final_periods[expire_slot.thread as usize] {
                break;
            }
            self.ops_per_expiration.pop_first();
            let op_info = self
                .operations
                .remove(&op_id)
                .expect("expected op presence in operations list");
            if !self.sorted_ops_per_thread[expire_slot.thread as usize].remove(&op_info.cursor) {
                panic!("expected op presence in sorted list")
            }
            removed_ops.insert(op_id);
        }

        // notify storage that pool has lost references to removed_ops
        self.storage.drop_operation_refs(&removed_ops);
    }

    /// Checks if an operation is relevant according to its thread and period validity range
    pub(crate) fn is_operation_relevant(&self, op_info: &OperationInfo) -> bool {
        // too old
        *op_info.validity_period_range.end() > self.last_cs_final_periods[op_info.thread as usize]
        // todo check if validity not started yet
    }

    /// Add a list of operations to the pool
    pub fn add_operations(&mut self, mut ops_storage: Storage) {
        let items = ops_storage
            .get_op_refs()
            .iter()
            .copied()
            .collect::<Vec<_>>();

        let mut added = PreHashSet::with_capacity(items.len());
        let mut removed = PreHashSet::with_capacity(items.len());

        // add items to pool
        {
            let ops = ops_storage.read_operations();
            for op_id in items {
                let op_info = OperationInfo::from_op(
                    ops.get(&op_id).expect(
                        "attempting to add operation to pool, but it is absent from storage",
                    ),
                    self.config.operation_validity_periods,
                    self.config.roll_price,
                );
                if !self.is_operation_relevant(&op_info) {
                    continue;
                }
                if let Ok(op_info) = self.operations.try_insert(op_info.id, op_info) {
                    if !self.sorted_ops_per_thread[op_info.thread as usize].insert(op_info.cursor) {
                        panic!("sorted ops should not contain the op at this point");
                    }
                    if !self.ops_per_expiration.insert((
                        Slot::new(*op_info.validity_period_range.end(), op_info.thread),
                        op_info.id,
                    )) {
                        panic!("expiration indexed ops should not contain the op at this point");
                    }
                    added.insert(op_info.id);
                }
            }
        }

        // prune excess operations
        self.sorted_ops_per_thread.iter_mut().for_each(|ops| {
            while ops.len() > self.config.max_operation_pool_size_per_thread {
                // the unrap below won't panic because the loop condition tests for non-emptines of self.operations
                let cursor = ops.pop_last().unwrap();
                let op_info = self
                    .operations
                    .remove(&cursor.get_id())
                    .expect("the operation should be in self.operations at this point");
                let end_slot = Slot::new(*op_info.validity_period_range.end(), op_info.thread);
                if !self.ops_per_expiration.remove(&(end_slot, op_info.id)) {
                    panic!("the operation should be in self.ops_per_expiration at this point");
                }
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
        let mut sequential_balance_cache: PreHashMap<Address, Amount> = Default::default();

        // iterate over pool operations in the right thread, from best to worst
        for cursor in self.sorted_ops_per_thread[slot.thread as usize].iter() {
            let op_info = self
                .operations
                .get(&cursor.get_id())
                .expect("the operation should be in self.operations at this point");

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
                .unexecuted_ops_among(&vec![op_info.id].into_iter().collect(), slot.thread)
                .is_empty()
            {
                continue;
            }

            // check sequential balance
            let creator_seq_balance = sequential_balance_cache
                .entry(op_info.creator_address)
                .or_insert_with(|| {
                    self.execution_controller
                        .get_final_and_candidate_sequential_balances(&[op_info.creator_address])
                        .get(0)
                        .unwrap_or(&(None, None))
                        .1
                        .unwrap_or_default()
                });

            if *creator_seq_balance < op_info.fee {
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
        let claim_ops: PreHashSet<OperationId> = op_ids.iter().copied().collect();
        let claimed_ops = res_storage.claim_operation_refs(&claim_ops);
        if claimed_ops.len() != claim_ops.len() {
            panic!("could not claim all operations from storage");
        }

        (op_ids, res_storage)
    }
}
