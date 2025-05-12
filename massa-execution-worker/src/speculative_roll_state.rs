// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::active_history::ActiveHistory;
use massa_execution_exports::ExecutionError;
use massa_final_state::FinalStateController;
use massa_models::address::ExecutionAddressCycleInfo;
use massa_models::{
    address::Address, amount::Amount, block_id::BlockId, prehash::PreHashMap, slot::Slot,
};
use massa_pos_exports::{DeferredCredits, PoSChanges, ProductionStats};
use num::rational::Ratio;
use parking_lot::RwLock;
use std::cmp::min;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Speculative state of the rolls
#[allow(dead_code)]
pub(crate) struct SpeculativeRollState {
    /// Thread-safe shared access to the final state. For reading only.
    final_state: Arc<RwLock<dyn FinalStateController>>,

    /// History of the outputs of recently executed slots.
    /// Slots should be consecutive, newest at the back.
    active_history: Arc<RwLock<ActiveHistory>>,

    /// List of changes to the state after settling roll sell/buy
    pub(crate) added_changes: PoSChanges,
}

impl SpeculativeRollState {
    /// Creates a new `SpeculativeRollState`
    ///
    /// # Arguments
    /// * `active_history`: thread-safe shared access the speculative execution history
    pub fn new(
        final_state: Arc<RwLock<dyn FinalStateController>>,
        active_history: Arc<RwLock<ActiveHistory>>,
    ) -> Self {
        SpeculativeRollState {
            final_state,
            active_history,
            added_changes: PoSChanges::default(),
        }
    }

    /// Returns the changes caused to the `SpeculativeRollState` since its creation,
    /// and resets their local value to nothing.
    pub fn take(&mut self) -> PoSChanges {
        std::mem::take(&mut self.added_changes)
    }

    /// Takes a snapshot (clone) of the changes caused to the `SpeculativeRollState` since its creation
    pub fn get_snapshot(&self) -> PoSChanges {
        self.added_changes.clone()
    }

    /// Resets the `SpeculativeRollState` to a snapshot (see `get_snapshot` method)
    pub fn reset_to_snapshot(&mut self, snapshot: PoSChanges) {
        self.added_changes = snapshot;
    }

    /// Internal function to retrieve the rolls of a given address
    fn get_rolls(&self, addr: &Address) -> u64 {
        self.added_changes
            .roll_changes
            .get(addr)
            .copied()
            .unwrap_or_else(|| {
                self.active_history
                    .read()
                    .fetch_roll_count(addr)
                    .unwrap_or_else(|| self.final_state.read().get_pos_state().get_rolls_for(addr))
            })
    }

    /// Add `roll_count` rolls to the buyer address.
    /// Validity checks must be performed _outside_ of this function.
    ///
    /// # Arguments
    /// * `buyer_addr`: address that will receive the rolls
    /// * `roll_count`: number of rolls it will receive
    pub fn add_rolls(&mut self, buyer_addr: &Address, roll_count: u64) {
        let count = self
            .added_changes
            .roll_changes
            .entry(*buyer_addr)
            .or_insert_with(|| {
                self.active_history
                    .read()
                    .fetch_roll_count(buyer_addr)
                    .unwrap_or_else(|| {
                        self.final_state
                            .read()
                            .get_pos_state()
                            .get_rolls_for(buyer_addr)
                    })
            });
        *count = count.saturating_add(roll_count);
    }

    /// Try to sell `roll_count` rolls from the seller address.
    ///
    /// # Arguments
    /// * `seller_addr`: address to sell the rolls from
    /// * `roll_count`: number of rolls to sell
    pub fn try_sell_rolls(
        &mut self,
        seller_addr: &Address,
        slot: Slot,
        roll_count: u64,
        periods_per_cycle: u64,
        thread_count: u8,
        roll_price: Amount,
    ) -> Result<(), ExecutionError> {
        // fetch the roll count from: current changes > active history > final state
        let owned_count = self.get_rolls(seller_addr);

        // verify that the seller has enough rolls to sell
        if owned_count < roll_count {
            return Err(ExecutionError::RollSellError(format!(
                "{} tried to sell {} rolls but only has {}",
                seller_addr, roll_count, owned_count
            )));
        }

        // compute deferred credit slot
        let cur_cycle = slot.get_cycle(periods_per_cycle);
        let target_slot = Slot::new_last_of_cycle(
            cur_cycle
                .checked_add(3)
                .expect("unexpected cycle overflow in try_sell_rolls"),
            periods_per_cycle,
            thread_count,
        )
        .expect("unexpected slot overflow in try_sell_rolls");

        // Note 1: Deferred credits are stored as absolute value
        let new_deferred_credits = self
            .get_address_deferred_credit_for_slot(seller_addr, &target_slot)
            .unwrap_or_default()
            .saturating_add(roll_price.saturating_mul_u64(roll_count));

        // Remove the rolls
        self.added_changes
            .roll_changes
            .insert(*seller_addr, owned_count.saturating_sub(roll_count));

        // Add deferred credits (reimbursement) corresponding to the sold rolls value
        self.added_changes
            .deferred_credits
            .insert(target_slot, *seller_addr, new_deferred_credits);

        Ok(())
    }

    /// Try to slash `roll_count` rolls from the given address. If not enough roll, slash
    /// the available amount and return the value.
    ///
    /// # Arguments
    /// * `addr`: address to slash the rolls from
    /// * `roll_count`: number of rolls to slash
    pub fn try_slash_rolls(
        &mut self,
        addr: &Address,
        roll_count: u64,
    ) -> Result<u64, ExecutionError> {
        // fetch the roll count
        let owned_count = self.get_rolls(addr);
        let roll_to_slash = min(roll_count, owned_count);

        match self.added_changes.roll_changes.get_mut(addr) {
            None => {
                // Rolls are in history or final state
                self.added_changes
                    .roll_changes
                    .insert(*addr, owned_count.saturating_sub(roll_to_slash));
                Ok(roll_to_slash)
            }
            Some(current_rolls) => {
                // Rolls are in added_changes
                *current_rolls = current_rolls.saturating_sub(roll_to_slash);
                Ok(roll_to_slash)
            }
        }
    }

    /// Try to slash `amount` credits from the given address. If not enough credits, slash
    /// the available amount and return the value.
    ///
    /// # Arguments
    /// * `addr`: address to slash the deferred credits from
    /// * `amount`: number of deferred credits to slash
    pub fn try_slash_deferred_credits(
        &mut self,
        slot: &Slot,
        addr: &Address,
        amount: &Amount,
    ) -> Amount {
        let credits = self.get_address_deferred_credits(addr, slot..);

        let mut remaining_to_slash = *amount;
        for (credit_slot, credit_amount) in credits.iter() {
            let to_slash = min(*credit_amount, remaining_to_slash);
            let new_deferred_credits = credit_amount.saturating_sub(to_slash);
            remaining_to_slash = remaining_to_slash.saturating_sub(to_slash);

            self.added_changes
                .deferred_credits
                .insert(*credit_slot, *addr, new_deferred_credits);
        }

        amount.saturating_sub(remaining_to_slash)
    }

    /// Update production statistics of an address.
    ///
    /// # Arguments
    /// * `creator`: the supposed creator
    /// * `slot`: current slot
    /// * `block_id`: id of the block (if some)
    pub fn update_production_stats(
        &mut self,
        creator: &Address,
        slot: Slot,
        block_id: Option<BlockId>,
    ) {
        let production_stats = self
            .added_changes
            .production_stats
            .entry(*creator)
            .or_default();
        if let Some(id) = block_id {
            production_stats.block_success_count =
                production_stats.block_success_count.saturating_add(1);
            self.added_changes.seed_bits.push(id.get_first_bit());
        } else {
            production_stats.block_failure_count =
                production_stats.block_failure_count.saturating_add(1);
            self.added_changes.seed_bits.push(slot.get_first_bit());
        }
    }

    /// Settle the production statistics at `slot`.
    ///
    /// IMPORTANT: This function should only be used at the end of a cycle.
    ///
    /// # Arguments:
    /// `slot`: the final slot of the cycle to compute
    pub fn settle_production_stats(
        &mut self,
        slot: &Slot,
        periods_per_cycle: u64,
        thread_count: u8,
        roll_price: Amount,
        max_miss_ratio: Ratio<u64>,
    ) -> Vec<(Address, Amount)> {
        #[allow(unused_mut)]
        let mut result = vec![];
        let cycle = slot.get_cycle(periods_per_cycle);

        let (production_stats, full) =
            self.get_production_stats_at_cycle(cycle, periods_per_cycle, thread_count, slot);
        if !full {
            panic!(
                "production stats were not fully ready when settle_production_stats was executed"
            )
        }

        let target_slot = Slot::new_last_of_cycle(
            cycle
                .checked_add(3)
                .expect("unexpected cycle overflow in settle_production_stats"),
            periods_per_cycle,
            thread_count,
        )
        .expect("unexpected slot overflow in settle_production_stats");

        let mut target_credits = PreHashMap::default();
        for (addr, stats) in production_stats {
            if !stats.is_satisfying(&max_miss_ratio) {
                let owned_count = self.get_rolls(&addr);
                if owned_count != 0 {
                    if let Some(amount) = roll_price.checked_mul_u64(owned_count) {
                        let new_deferred_credits = self
                            .get_address_deferred_credit_for_slot(&addr, &target_slot)
                            .unwrap_or_default()
                            .saturating_add(amount);
                        target_credits.insert(addr, new_deferred_credits);
                        self.added_changes.roll_changes.insert(addr, 0);

                        #[cfg(feature = "execution-info")]
                        result.push((addr, amount));
                    }
                }
            }
        }
        if !target_credits.is_empty() {
            let mut credits = DeferredCredits::new();
            credits.credits.insert(target_slot, target_credits);
            self.added_changes.deferred_credits.extend(credits);
        }

        result
    }

    /// Get deferred credits of an address starting from a given slot
    pub fn get_address_deferred_credits<R>(
        &self,
        address: &Address,
        slot_range: R,
    ) -> BTreeMap<Slot, Amount>
    where
        R: std::ops::RangeBounds<Slot> + Clone,
    {
        let mut res: HashMap<Slot, Amount> = HashMap::default();

        // get added values
        for (slot, addr_amount) in self
            .added_changes
            .deferred_credits
            .credits
            .range(slot_range.clone())
        {
            if let Some(amount) = addr_amount.get(address) {
                res.entry(*slot).or_insert(*amount);
            };
        }

        // get values from active history, backwards
        {
            let hist = self.active_history.read();
            for hist_item in hist.0.iter().rev() {
                for (slot, addr_amount) in hist_item
                    .state_changes
                    .pos_changes
                    .deferred_credits
                    .credits
                    .range(slot_range.clone())
                {
                    if let Some(amount) = addr_amount.get(address) {
                        res.entry(*slot).or_insert(*amount);
                    };
                }
            }
        }

        // get values from final state
        {
            let final_state = self.final_state.read();
            for (slot, addr_amount) in final_state
                .get_pos_state()
                .get_deferred_credits_range(slot_range, Some(address))
                .credits
            {
                if let Some(amount) = addr_amount.get(address) {
                    res.entry(slot).or_insert(*amount);
                };
            }
        }

        res.into_iter().filter(|(_s, v)| !v.is_zero()).collect()
    }

    /// Gets the deferred credits for a given address that will be credited at a given slot
    fn get_address_deferred_credit_for_slot(&self, addr: &Address, slot: &Slot) -> Option<Amount> {
        // search in the added changes
        if let Some(v) = self
            .added_changes
            .deferred_credits
            .get_address_credits_for_slot(addr, slot)
        {
            return Some(v);
        }

        // search in the history
        if let Some(v) = self
            .active_history
            .read()
            .get_address_deferred_credit_for(addr, slot)
        {
            return Some(v);
        }

        // search in the final state
        if let Some(v) = self
            .final_state
            .read()
            .get_pos_state()
            .get_address_credits_for_slot(addr, slot)
        {
            return Some(v);
        }

        None
    }

    /// Get the production statistics for a given address at a given cycle.
    pub fn get_address_cycle_infos(
        &self,
        address: &Address,
        periods_per_cycle: u64,
        cur_slot: Slot,
    ) -> Vec<ExecutionAddressCycleInfo> {
        let mut res: Vec<ExecutionAddressCycleInfo> = Vec::new();

        // lock final state
        let final_state = self.final_state.read();

        // add finals
        final_state
            .get_pos_state()
            .cycle_history_cache
            .iter()
            .for_each(|c| {
                let mut cur_item = ExecutionAddressCycleInfo {
                    cycle: c.0,
                    is_final: c.1,
                    ok_count: 0,
                    nok_count: 0,
                    active_rolls: None, // will be filled afterwards
                };
                if let Some(prod_stats) = final_state
                    .get_pos_state()
                    .get_production_stats_for_address(c.0, address)
                {
                    cur_item.ok_count = prod_stats.block_success_count;
                    cur_item.nok_count = prod_stats.block_failure_count;
                }
                res.push(cur_item);
            });

        // add active history
        // note that a last cycle might overlap between final and active histories
        {
            let hist = self.active_history.read();
            for hist_elt in &hist.0 {
                let hist_cycle = hist_elt.slot.get_cycle(periods_per_cycle);

                // insert a new item if necessary
                if !res.last().map(|v| v.cycle == hist_cycle).unwrap_or(false) {
                    res.push(ExecutionAddressCycleInfo {
                        cycle: hist_cycle,
                        is_final: false,
                        ok_count: 0,
                        nok_count: 0,
                        active_rolls: None, // will be filled afterwards
                    });
                }

                // accumulate active stats
                if let Some(stats) = hist_elt
                    .state_changes
                    .pos_changes
                    .production_stats
                    .get(address)
                {
                    let cur_item = res
                        .last_mut()
                        .expect("last item of the result should exist here");
                    cur_item.ok_count = cur_item.ok_count.saturating_add(stats.block_success_count);
                    cur_item.nok_count =
                        cur_item.nok_count.saturating_add(stats.block_failure_count);
                }
            }
        }

        // take into account added changes
        {
            // get current cycle
            let cur_cycle = cur_slot.get_cycle(periods_per_cycle);

            // insert a new item if necessary
            if !res.last().map(|v| v.cycle == cur_cycle).unwrap_or(false) {
                res.push(ExecutionAddressCycleInfo {
                    cycle: cur_cycle,
                    is_final: false,
                    ok_count: 0,
                    nok_count: 0,
                    active_rolls: None, // will be filled afterwards
                });
            }

            // accumulate added stats
            if let Some(stats) = self.added_changes.production_stats.get(address) {
                let cur_item = res
                    .last_mut()
                    .expect("last item of the result should exist here");
                cur_item.ok_count = cur_item.ok_count.saturating_add(stats.block_success_count);
                cur_item.nok_count = cur_item.nok_count.saturating_add(stats.block_failure_count);
            }
        }

        // add active roll counts
        for itm in res.iter_mut() {
            itm.active_rolls = final_state
                .get_pos_state()
                .get_address_active_rolls(address, itm.cycle);
        }

        res
    }

    /// Get the production statistics for a given cycle.
    /// Returns a 2nd boolean result indicating whether the cycle was fetched fully and successfully, or just partially.
    pub fn get_production_stats_at_cycle(
        &self,
        cycle: u64,
        periods_per_cycle: u64,
        thread_count: u8,
        cur_slot: &Slot,
    ) -> (PreHashMap<Address, ProductionStats>, bool) {
        let mut accumulated_stats: PreHashMap<Address, ProductionStats> = Default::default();
        let mut underflow;
        let mut overflow;

        // search in active history
        {
            let hist = self.active_history.read();
            let (range, loc_underflow, loc_overflow) =
                hist.find_cycle_indices(cycle, periods_per_cycle, thread_count);
            underflow = loc_underflow;
            overflow = loc_overflow;
            for idx in range {
                for (addr, stats) in &hist.0[idx].state_changes.pos_changes.production_stats {
                    accumulated_stats
                        .entry(*addr)
                        .and_modify(|cur| cur.extend(stats))
                        .or_insert_with(|| *stats);
                }
            }
        }

        // on overflow, accumulate added changes
        if overflow && cur_slot.get_cycle(periods_per_cycle) == cycle {
            let last_slot_of_target_cycle =
                Slot::new_last_of_cycle(cycle, periods_per_cycle, thread_count)
                    .expect("could not get last slot of cycle");
            if cur_slot <= &last_slot_of_target_cycle {
                for (addr, stats) in &self.added_changes.production_stats {
                    accumulated_stats
                        .entry(*addr)
                        .and_modify(|cur| cur.extend(stats))
                        .or_insert_with(|| *stats);
                }
                if cur_slot == &last_slot_of_target_cycle {
                    overflow = false;
                }
            }
        }

        // on underflow, accumulate final state
        if underflow {
            let final_state = self.final_state.read();
            if let Some(final_stats) = final_state.get_pos_state().get_all_production_stats(cycle) {
                for (addr, stats) in final_stats {
                    accumulated_stats
                        .entry(addr)
                        .and_modify(|cur| cur.extend(&stats))
                        .or_insert_with(|| stats);
                }
                underflow = false;
            }
        }

        (accumulated_stats, !underflow && !overflow)
    }

    /// Take the non-zero deferred credits at or before `slot`.
    /// Set them to zero in the speculative state.
    ///
    /// # Arguments
    /// * `slot`: associated slot of the deferred credits to be executed
    pub fn take_unexecuted_deferred_credits(&mut self, slot: &Slot) -> DeferredCredits {
        // NOTE: Deferred credits are overridden. Zeros will be deleted at finality.

        // get final deferred credits
        let mut credits = self
            .final_state
            .read()
            .get_pos_state()
            .get_deferred_credits_range(..=slot, None);

        // fetch active history deferred credits
        credits.extend(
            self.active_history
                .read()
                .get_all_deferred_credits_until(slot),
        );

        // added deferred credits
        credits.extend(self.added_changes.deferred_credits.get_slot_range(..=slot));

        // filter out zeros
        credits.remove_zeros();

        // set all the taken ones to zero in added_changes
        credits.for_each(|slot, address, _amount| {
            self.added_changes
                .deferred_credits
                .insert(*slot, *address, Amount::zero());
        });

        // return taken credits
        credits
    }
}
