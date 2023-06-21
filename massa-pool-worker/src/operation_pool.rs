//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    address::Address,
    amount::Amount,
    operation::OperationId,
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
    slot::Slot,
    timeslots::get_latest_block_slot_at_timestamp,
};
use massa_pool_exports::{PoolChannels, PoolConfig};
use massa_storage::Storage;
use massa_time::MassaTime;
use massa_wallet::Wallet;
use parking_lot::RwLock;
use std::{cmp::max, cmp::Ordering, cmp::PartialOrd, collections::BTreeSet, sync::Arc};
use tracing::debug;

use crate::types::OperationInfo;

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
            .map(|(t, p)| Slot::new(*p, t as u8))
            .min()
            .expect("empty last_vs_final_periods in operation pool");
        // max slot for PoS draw search = the slot after now() + max future start delay + margin
        let max_slot = get_latest_block_slot_at_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            now.saturating_add(self.config.operation_max_future_start_delay)
                .saturating_add(self.config.t0.saturating_mul(2)),
        )
        .unwrap_or(Some(Slot::max(self.config.thread_count)))
        .unwrap_or(min_slot);
        let max_slot = max(max_slot, min_slot);

        // search for all our PoS draws in the interval of interest
        let mut pos_draws = BTreeSet::new();
        let addrs = self.wallet.read().keys.keys().copied().collect::<Vec<_>>();
        for addr in addrs {
            let (d, _) = self
                .channels
                .selector
                .get_address_selections(&addr, min_slot, max_slot)
                .expect("could not get PoS draws");
            pos_draws.extend(d.into_iter());
        }
        // retain only the ones that are strictly after the last final slot of their thread
        pos_draws.retain(|s| s.period > self.last_cs_final_periods[s.thread as usize]);
        pos_draws
    }

    /// Returns the list of executed ops with a boolean indicating whether they are executed as final.
    fn get_execution_statuses(&self) -> PreHashMap<OperationId, bool> {
        let op_ids: Vec<OperationId> = self.sorted_ops.iter().map(|op_info| op_info.id).collect();
        self.channels
            .execution_controller
            .get_ops_exec_status(&op_ids)
            .into_iter()
            .zip(op_ids.into_iter())
            .filter_map(
                |((spec_status, final_status), op_id)| match (spec_status, final_status) {
                    (Some(_), Some(_)) => Some((op_id, true)),
                    (Some(_), None) => Some((op_id, false)),
                    _ => None,
                },
            )
            .collect()
    }

    /// Get the candidate balances of the addresses sending the ops.
    /// Addresses that don't exist are not returned.
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
            .filter_map(|((_, c_balance), addr)| c_balance.map(|v| (addr, v)))
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
            // filter out ops that use too much resources
            let mut retain = (op_info.max_gas <= self.config.max_block_gas)
                && (op_info.size <= self.config.max_block_size as usize);

            // filter out ops that are not valid during our PoS draws
            if retain {
                retain = pos_draws.iter().any(|slot| {
                    op_info.thread == slot.thread
                        && op_info.validity_period_range.contains(&slot.period)
                });
            }

            // filter out ops that have been executed in final or candidate slots
            // TODO: in the re-execution followup, we should only filter out final-executed ops here (exec_status == Some(true))
            if retain {
                retain = !exec_statuses.contains_key(&op_info.id);
            }

            // filter out ops that spend more than the sender's balance
            if retain {
                retain = match sender_balances.get(&op_info.creator_address) {
                    Some(v) => &op_info.max_spending <= v,
                    None => false, // filter out ops for which the sender does not exist
                };
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
                        .unwrap_or_default()
                });
            match balance.checked_sub(op_info.max_spending) {
                Some(v) => {
                    *balance = v;
                    true
                }
                None => {
                    removed.insert(op_info.id);
                    false
                }
            }
        });
        // drop from storage
        self.storage.drop_operation_refs(&removed);
    }

    /// Truncates the container to the max allowed size
    fn truncate_container(&mut self) {
        if self.sorted_ops.len() > self.config.max_operation_pool_size {
            let mut removed = PreHashSet::default();
            for op_info in self
                .sorted_ops
                .iter()
                .skip(self.config.max_operation_pool_size)
            {
                removed.insert(op_info.id);
            }
            self.sorted_ops
                .truncate(self.config.max_operation_pool_size);
            // drop from storage
            self.storage.drop_operation_refs(&removed);
        }
    }

    /// Score the operations
    fn score_operations(
        &self,
        _exec_statuses: &PreHashMap<OperationId, bool>,
        pos_draws: &BTreeSet<Slot>,
    ) -> PreHashMap<OperationId, f32> {
        let now = MassaTime::now().expect("could not get current time");
        let now_period = get_latest_block_slot_at_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            now,
        )
        .expect("could not get current slot")
        .map_or(0, |s| s.period);

        let mut scores = PreHashMap::with_capacity(self.sorted_ops.len());
        for op_info in &self.sorted_ops {
            // fee factor
            // (we add 1 to still sort zero-fee ops)
            let fee_factor = op_info.fee.to_raw().saturating_add(1) as f32;

            // size score:
            //    0% of block size => score 1
            //    100% of block size => score 0
            let size_score = 1.0 - (op_info.size as f32) / (self.config.max_block_size as f32);

            // gas score:
            //    0% of block gas => score 1
            //    100% of block gas => score 0
            let gas_score = 1.0 - (op_info.max_gas as f32) / (self.config.max_block_gas as f32);

            // general resource score (mean of gas and size scores)
            let epsilon_resource_factor = 0.0001; // avoids zero score when gas and size are a perfect fit in the block
            let resource_factor = (epsilon_resource_factor + size_score + gas_score)
                / (2.0 + epsilon_resource_factor);

            // inclusion probability factor
            //    If we are selected to produce a block in a long time,
            //    there is exponential likelihood that someone includes the op before us.
            let tau_inclusion = 2.0; // exponential decay factor
            let earliest_inclusion_opportunity = pos_draws.iter().find_map(|s| {
                if s.thread == op_info.thread
                    && op_info.validity_period_range.contains(&s.period)
                    && s.period >= now_period.saturating_sub(1)
                {
                    Some(s.period)
                } else {
                    None
                }
            });
            let inclusion_factor =
                if let Some(earliest_inclusion_opportunity) = earliest_inclusion_opportunity {
                    // compute the number of slots other stakers have available to include the op before we do
                    let foreign_opportunities = earliest_inclusion_opportunity.saturating_sub(max(
                        now_period.saturating_add(1),
                        *op_info.validity_period_range.start(),
                    ));
                    (-(foreign_opportunities as f32) / tau_inclusion).exp()
                } else {
                    // no inclusion opportunity => score 0
                    0.0
                };

            /* TODO: re-execution followup
            // If the op was executed previously, there is still an exponentially decaying chance of its block being cancelled
            // so that it can be reincluded.
            // We approximate it with a constant factor for simplicity since we don't have the inclusion slot for now.
            let reexecution_penalty = 1.0 / 1000.0; // re-execution penalty factor
            let reexecution_factor = if exec_statuses.contains_key(&op_info.id) {
                // executed previously
                reexecution_penalty
            } else {
                // not executed previously => score 1
                1.0
            };
            */

            // compute the score as being the product of all the factors and the fee
            let score = fee_factor * resource_factor * inclusion_factor;
            //  * reexecution_factor; // TODO: re-execution followup

            // store the score
            scores.insert(op_info.id, score);
        }
        scores
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
                .partial_cmp(&scores.get(&op1.id))
                .unwrap_or(Ordering::Equal)
        });

        // eliminate balance overflows in sorted ops
        self.eliminate_balance_overflows(&sender_balances);

        // eliminate container size overflows
        self.truncate_container();
    }

    /// Get the number of stored elements
    pub fn len(&self) -> usize {
        self.sorted_ops.len()
    }

    /// Checks whether an element is stored in the pool.
    pub fn contains(&self, id: &OperationId) -> bool {
        self.storage.get_op_refs().contains(id)
    }

    /// notify of new final slot
    pub(crate) fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        // update internal final slot counter
        self.last_cs_final_periods = final_cs_periods.to_vec();
        debug!(
            "notified of new final consensus periods: {:?}",
            self.last_cs_final_periods
        );
    }

    /// Add a list of operations to the end of the pool.
    /// They will be cleaned up at the next refresh.
    pub(crate) fn add_operations(&mut self, mut ops_storage: Storage) {
        let new_op_ids = ops_storage.get_op_refs() - self.storage.get_op_refs();
        {
            let ops = ops_storage.read_operations();
            for new_op_id in &new_op_ids {
                let op = ops
                    .get(new_op_id)
                    .expect("operation not found in storage but listed as owned");
                self.sorted_ops.push(OperationInfo::from_op(
                    op,
                    self.config.operation_validity_periods,
                    self.config.roll_price,
                    self.config.thread_count,
                ));
            }
        }

        // This will add the new ops to the storage without taking locks.
        // It just take the local references from `ops_storage` if they are not in `self.storage` yet.
        // If the objects are already in `self.storage` the references in ops_storage it will not add them to `self.storage` and
        // at the end of the scope ops_storage will be dropped and so the references will be only in `self.storage`
        // If the object wasn't in `self.storage` the reference will be transferred and so the number of owners doesn't change
        // and when we will drop `ops_storage` it doesn't have the references anymore and so doesn't drop those objects.
        self.storage.extend(ops_storage.split_off(
            &Default::default(),
            &new_op_ids,
            &Default::default(),
        ));
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

        // iterate over pool operations in the right thread, from best to worst
        for op_info in &self.sorted_ops {
            // if we have reached the maximum number of operations, stop
            if remaining_ops == 0 {
                break;
            }

            // check thread
            if op_info.thread != slot.thread {
                continue;
            }

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

            // here we consider the operation as accepted
            op_ids.push(op_info.id);

            // update remaining block space
            remaining_space -= op_info.size;

            // update remaining block gas
            remaining_gas -= op_info.max_gas;

            // update remaining number of operations
            remaining_ops -= 1;
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
