//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_execution_exports::ExecutionController;
use massa_models::{
    address::Address,
    amount::Amount,
    operation::OperationId,
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
    slot::Slot,
    timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp},
};
use massa_pool_exports::{PoolChannels, PoolConfig};
use massa_storage::Storage;
use massa_time::MassaTime;
use massa_wallet::Wallet;
use parking_lot::RwLock;
use std::{
    cmp::PartialOrd,
    cmp::{Ordering, Reverse},
    collections::BTreeSet,
    sync::Arc,
};
use tracing::{debug, trace};

use crate::types::{OperationInfo, PoolOperationCursor};

pub struct OperationPool {
    /// configuration
    config: PoolConfig,

    /// operations map
    sorted_ops: Vec<OperationInfo>,

    /// storage instance
    pub(crate) storage: Storage,

    /// last consensus final periods, per thread
    last_cs_final_periods: Vec<u64>,

    /// channels used by the pool worker
    channels: PoolChannels,

    /// staking wallet, to know which addresses we are using to stake
    wallet: Arc<RwLock<Wallet>>,
}

impl OperationPool {
    pub fn init(
        config: PoolConfig,
        storage: &Storage,
        channels: PoolChannels,
        wallet: Arc<RwLock<Wallet>>,
    ) -> Self {
        OperationPool {
            sorted_ops: Default::default(),
            last_cs_final_periods: vec![0u64; config.thread_count as usize],
            config,
            storage: storage.clone_without_refs(),
            channels,
            wallet,
        }
    }

    /// Get the relevant PoS draws of our staking addresses
    fn get_pos_draws(&mut self) -> BTreeSet<Slot> {
        let now = MassaTime::now().expect("could not get current time");

        // min slot for PoS draw search = the earliest final slot
        let min_slot = self
            .last_cs_final_periods
            .iter()
            .enumerate()
            .map(|(t, p)| Slot::new(p, t as u8))
            .min()
            .expect("empty last_vs_final_periods in operation pool");
        // max slot for PoS draw search = the slot after now() + max future start delay + margin
        let max_slot = get_latest_block_slot_at_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            now.saturating_add(self.config.operation_max_future_start_delay)
                .saturating_add(self.config.t0 * 2),
        )
        .unwrap_or(Some(Slot::max(self.config.thread_count)))
        .unwrap_or(min_slot);
        let max_slot = std::cmp::max(max_slot, min_slot);

        // search for all our PoS draws
        let pos_draws = BTreeSet::new();
        let addrs = self.wallet.read().keys.keys().copied().collect::<Vec<_>>();
        for addr in addrs {
            let (d, _) = self
                .channels
                .selector
                .get_address_selections(&addr, min_slot, max_slot)
                .expect("could not get PoS draws");
            self.pos_draws.extend(d.into_iter());
        }
        // retain only the ones that are strictly after the last final slot of their thread
        self.pos_draws
            .retain(|s| s.period > self.last_cs_final_periods[s.thread as usize]);
        pos_draws
    }

    /// Returns the list of executed ops with a boolean indicating whether they are executed as final.
    fn get_execution_statuses(&self) -> PreHashMap<OperationId, bool> {
        /// TODO make it lighter by not querying ALL executed ops !
        /// https://github.com/massalabs/massa/issues/4075
        let (speculative_status, final_status) =
            self.channels.execution_controller.get_op_exec_status();
        self.sorted_ops
            .iter()
            .filter_map(|op_info| {
                if final_status.contains_key(&op_info.id) {
                    Some((op_info.id, true))
                } else if speculative_status.contains_key(&op_info.id) {
                    Some((op_info.id, false))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get the candidate balances of the addresses sending the ops.
    fn get_sender_balances(&self) -> PreHashMap<Address, Amount> {
        let addrs: Vec<Address> = self
            .sorted_ops
            .iter()
            .map(|op_info| op_info.creator_address)
            .collect::<PreHashSet<Address>>()
            .into_iter()
            .collect();
        let ret = self
            .channels
            .execution_controller
            .get_final_and_candidate_balance(&addrs);
        ret.into_iter()
            .zip(addrs.into_iter())
            .map(|((_, c_balance), addr)| (addr, balance))
            .collect()
    }

    /// Filter out ops that are not of interest.
    fn prefilter_ops(
        &mut self,
        exec_statuses: &PreHashMap<OperationId, bool>,
        pos_draws: &BTreeSet<Slot>,
        sender_balances: &PreHashMap<Address, Amount>,
    ) {
        let mut removed = PreHashSet::default();
        self.sorted_ops.retain(|op_info| {
            // filter out ops that are not valid during our PoS draws
            let mut retain = pos_draws.iter().any(|slot| {
                op_info.thread == slot.thread
                    && op_info.validity_period_range.contains(&slot.period)
            });

            // filter out ops that have been executed in final slots
            if retain {
                retain = (exec_statuses.get(&op_info.id) != Some(&true));
            }

            // filter out ops that spend more than the sender's balance
            if retain {
                retain = sender_balances
                    .get(&op_info.creator_address)
                    .copied()
                    .unwrap_or(0)
                    >= op_info.max_spending;
            }

            if !retain {
                removed.insert(op_info.id);
                return false;
            }
            true
        });
        // drop from storage
        self.storage.drop_operation_refs(&removed);
    }

    /// Eliminate all operations that would cause a sender balance overflow.
    /// Assumes that the ops are sorted by ascending score.
    fn eliminate_balance_overflows(&mut self, sender_balances: &PreHashMap<Address, Amount>) {
        let mut balance_cache = PreHashMap::default();
        let mut removed = PreHashSet::default();
        self.sorted_ops.retain(|op_info| {
            let balance = balance_cache
                .entry(op_info.creator_address)
                .or_insert_with(|| {
                    sender_balances
                        .get(&op_info.creator_address)
                        .copied()
                        .unwrap_or(0)
                });
            match *balance.saturating_sub(op_info.max_spending) {
                Some(v) => {
                    *balance = v;
                    true
                }
                None => {
                    removed.insert(op_info.id);
                    false
                }
            }
            true
        });
        // drop from storage
        self.storage.drop_operation_refs(&removed);
    }

    /// Truncates the container to the max allowed size
    fn truncate_container(&mut self) {
        if self.sorted_ops.len() > self.config.max_operation_pool_size {
            let mut removed = PreHashSet::default();
            for op_info in self.sorted_ops.iter().skip(self.config.max_container_size) {
                removed.insert(op_info.id);
            }
            self.sorted_ops.truncate(self.config.max_container_size);
            // drop from storage
            self.storage.drop_operation_refs(&removed);
        }
    }

    /// Score the operations
    fn score_operations(
        &self,
        exec_statuses: &PreHashMap<OperationId, bool>,
        pos_draws: &BTreeSet<Slot>,
    ) -> PreHashMap<OperationId, f32> {
        // TODO
        unimplemented!()
    }

    /// Refresh the pool
    pub(crate) fn refresh(&mut self) {
        // get PoS draws
        let pos_draws = self.get_pos_draws();

        // get execution statuses
        let exec_statuses = self.get_execution_statuses();

        // get sender balances
        let sender_balances = self.get_sender_balances();

        // pre-filter to eliminate obviously uninteresting ops
        self.prefilter_ops(&exec_statuses, &pos_draws, &sender_balances);

        // score operations
        let scores = self.score_operations(&exec_statuses, &pos_draws);

        // sort by score
        self.sorted_ops.sort_unstable_by(|op1, op2| {
            // note1: scores are float => we need to use partial_cmp.
            // note2: operands are reversed to sort from highest to lowest !
            scores
                .get(&op2.id)
                .partial_cmp(scores.get(&op1.id))
                .unwrap_or(Ordering::Equal)
        });

        // eliminate balance overflows in sorted ops
        self.eliminate_balance_overflows(&sender_balances);

        // eliminate container size overflows
        self.truncate_container();
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
    pub(crate) fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        // update internal final slot counter
        self.last_cs_final_periods = final_cs_periods.to_vec();
        debug!(
            "notified of new final consensus periods: {:?}",
            self.last_cs_final_periods
        );
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
    pub(crate) fn add_operations(&mut self, mut ops_storage: Storage) {
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
                let op = ops
                    .get(&op_id)
                    .expect("attempting to add operation to pool, but it is absent from storage");
                // Broadcast operation to active channel subscribers.
                if self.config.broadcast_enabled {
                    if let Err(err) = self.channels.operation_sender.send(op.clone()) {
                        trace!(
                            "error, failed to broadcast operation with id {} due to: {}",
                            op.id.clone(),
                            err
                        );
                    }
                }

                let op_info = OperationInfo::from_op(
                    op,
                    self.config.operation_validity_periods,
                    self.config.roll_price,
                    self.config.thread_count,
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
                // the unwrap below won't panic because the loop condition tests for non-emptines of self.operations
                let cursor = ops.pop_last().unwrap();
                let op_info = self
                    .operations
                    .remove(&cursor.get_id())
                    .expect("the operation should be in self.operations at this point");
                let end_slot = Slot::new(*op_info.validity_period_range.end(), op_info.thread);
                if !self.ops_per_expiration.remove(&(end_slot, op_info.id)) {
                    panic!("the operation should be in self.ops_per_expiration at this point");
                }
                removed.insert(op_info.id);
            }
        });

        // This will add the new ops to the storage without taking locks.
        // It just take the local references from `ops_storage` if they are not in `self.storage` yet.
        // If the objects are already in `self.storage` the references in ops_storage it will not add them to `self.storage` and
        // at the end of the scope ops_storage will be dropped and so the references will be only in `self.storage`
        // If the object wasn't in `self.storage` the reference will be transferred and so the number of owners doesn't change
        // and when we will drop `ops_storage` it doesn't have the references anymore and so doesn't drop those objects.
        self.storage.extend(ops_storage.split_off(
            &Default::default(),
            &added,
            &Default::default(),
        ));

        // Clean the removed operations from storage.
        self.storage.drop_operation_refs(&removed);
    }

    /// get operations for block creation
    ///
    /// Searches the available operations, and selects the sub-set of operations that:
    /// - fit inside the block
    /// - is the most profitable for block producer
    pub fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage) {
        // init list of selected operation IDs
        let mut op_ids = Vec::new();

        // init remaining space
        let mut remaining_space = self.config.max_block_size as usize;
        // init remaining gas
        let mut remaining_gas = self.config.max_block_gas;
        // init remaining number of operations
        let mut remaining_ops = self.config.max_operations_per_block;
        // cache of balances
        let mut balance_cache: PreHashMap<Address, Amount> = Default::default();

        // iterate over pool operations in the right thread, from best to worst
        for cursor in self.sorted_ops_per_thread[slot.thread as usize].iter() {
            // if we have reached the maximum number of operations, stop
            if remaining_ops == 0 {
                break;
            }
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
            // TODO batch this
            if self
                .execution_controller
                .unexecuted_ops_among(&vec![op_info.id].into_iter().collect(), slot.thread)
                .is_empty()
            {
                continue;
            }

            // check balance
            //TODO: It's a weird behaviour because if the address is created afterwards this operation will be executed
            // and also it spams the pool maybe we should just try to put the operation if there is no balance and 0 gas price
            // and the execution will throw an error
            let creator_balance =
                if let Some(amount) = balance_cache.get_mut(&op_info.creator_address) {
                    amount
                } else if let Some(balance) = self
                    .execution_controller
                    .get_final_and_candidate_balance(&[op_info.creator_address])
                    .get(0)
                    .map(|balances| balances.1.or(balances.0))
                    && let Some(final_amount) = balance {
                        balance_cache
                        .entry(op_info.creator_address)
                        .or_insert(final_amount)
                } else {
                    continue;
                };

            if *creator_balance < op_info.fee {
                continue;
            }

            // here we consider the operation as accepted
            op_ids.push(op_info.id);

            // update remaining block space
            remaining_space -= op_info.size;

            // update remaining block gas
            remaining_gas -= op_info.max_gas;

            // update remaining number of operations
            remaining_ops -= 1;

            // update balance cache
            *creator_balance = creator_balance.saturating_sub(op_info.max_spending);
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
