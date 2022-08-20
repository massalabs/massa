use massa_models::{
    constants::{default::POS_SAVED_CYCLES, PERIODS_PER_CYCLE},
    prehash::Map,
    Address, Amount, Slot, THREAD_COUNT,
};

use crate::{CycleInfo, PoSChanges, PoSFinalState, ProductionStats, SelectorController};

impl PoSFinalState {
    /// Used to give the selector controller to `PoSFinalState` when it has been created
    pub fn give_selector_controller(&mut self, selector: Box<dyn SelectorController>) {
        self.selector = Some(selector);
    }

    /// Technical specification of apply_changes:
    ///
    /// set self.last_final_slot = C
    /// if cycle C is absent from self.cycle_history:
    ///     push a new empty CycleInfo at the back of self.cycle_history and set its cycle = C
    ///     pop_front from cycle_history until front() represents cycle C-4 or later (not C-3 because we might need older endorsement draws on the limit between 2 cycles)
    /// for the cycle C entry of cycle_history:
    ///     extend seed_bits with changes.seed_bits
    ///     extend roll_counts with changes.roll_changes
    ///         delete all entries from roll_counts for which the roll count is zero
    ///     add each element of changes.production_stats to the cycle's production_stats
    /// for each changes.deferred_credits targeting cycle Ct:
    ///     overwrite self.deferred_credits entries of cycle Ct in cycle_history with the ones from change
    ///         remove entries for which Amount = 0
    /// if slot S was the last of cycle C:
    ///     set complete=true for cycle C in the history
    ///     compute the seed hash and notifies the PoSDrawer for cycle C+3
    ///
    pub fn settle_slot(&mut self, changes: PoSChanges, slot: Slot) {
        // compute the current cycle from the given slot
        let cycle = slot.get_cycle(PERIODS_PER_CYCLE);

        // if cycle C is absent from self.cycle_history:
        // push a new empty CycleInfo at the back of self.cycle_history and set its cycle = C
        // pop_front from cycle_history until front() represents cycle C-4 or later
        // (not C-3 because we might need older endorsement draws on the limit between 2 cycles)
        if let Some(info) = self.cycle_history.iter().last() {
            if info.cycle != cycle {
                self.cycle_history.push_back(CycleInfo {
                    cycle,
                    ..Default::default()
                });
                // add 1 for the current cycle and 1 for bootstrap safety
                while self.cycle_history.len() as u64 > POS_SAVED_CYCLES + 2 {
                    self.cycle_history.pop_front();
                }
            }
        } else {
            self.cycle_history.push_back(CycleInfo {
                cycle,
                ..Default::default()
            });
        }

        let current = self
            .cycle_history
            .back_mut()
            .expect("expected a non-empty cycle history");

        // extend seed_bits with changes.seed_bits
        current.rng_seed.extend(changes.seed_bits);

        // extend roll counts
        current.roll_counts.extend(changes.roll_changes);
        current.roll_counts.retain(|_, &mut count| count != 0);

        // extend production stats
        for (addr, stats) in changes.production_stats {
            current
                .production_stats
                .entry(addr)
                .and_modify(|cur| cur.extend(&stats))
                .or_insert(stats);
        }

        // extent deferred_credits with changes.deferred_credits
        // remove zero-valued credits
        self.deferred_credits
            .nested_extend(changes.deferred_credits);
        self.deferred_credits.remove_zeros();

        // feed the cycle if it is complete
        // if slot S was the last of cycle C:
        // set complete=true for cycle C in the history
        // notify the PoSDrawer for cycle C+3
        if slot.is_last_of_cycle(PERIODS_PER_CYCLE, THREAD_COUNT) {
            current.complete = true;
            self.selector
                .as_ref()
                .expect("critical: SelectorController is missing from PoSFinalState")
                .feed_cycle(current.clone())
                .expect(
                    "critical: could not feed complete cycle too SelectorController: channel down",
                );
        }
    }

    /// Retrieves the amount of rolls a given address has at the latest cycle
    pub fn get_rolls_for(&self, addr: &Address) -> u64 {
        self.cycle_history
            .back()
            .and_then(|info| info.roll_counts.get(addr).cloned())
            .unwrap_or_default()
    }

    /// Retrieves the amount of rolls a given address has at a given cycle
    pub fn get_address_active_rolls(&self, addr: &Address, cycle: u64) -> Option<u64> {
        // get lookback cycle index
        let lookback_cycle = cycle.saturating_sub(3);
        let lookback_index = match self.get_cycle_index(lookback_cycle) {
            Some(idx) => idx,
            None => return None,
        };
        // get rolls
        self.cycle_history[lookback_index]
            .roll_counts
            .get(addr)
            .cloned()
    }

    /// Retrives every deferred credit of the given slot
    pub fn get_deferred_credits_at(&self, slot: &Slot) -> Map<Address, Amount> {
        self.deferred_credits
            .0
            .get(slot)
            .cloned()
            .unwrap_or_default()
    }

    /// Retrives the productions statistics for all addresses on a given cycle
    pub fn get_all_production_stats(&self, cycle: u64) -> Option<&Map<Address, ProductionStats>> {
        self.get_cycle_index(cycle)
            .and_then(|idx| Some(&self.cycle_history[idx].production_stats))
    }

    /// Gets the index of a cycle in history
    pub fn get_cycle_index(&self, cycle: u64) -> Option<usize> {
        let first_cycle = match self.cycle_history.front() {
            Some(c) => c.cycle,
            None => return None, // history empty
        };
        if cycle < first_cycle {
            return None; // in the past
        }
        let index: usize = match (cycle - first_cycle).try_into() {
            Ok(v) => v,
            Err(_) => return None, // usize overflow
        };
        if index >= self.cycle_history.len() {
            return None; // in the future
        }
        Some(index)
    }
}
