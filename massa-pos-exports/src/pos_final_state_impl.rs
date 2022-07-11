use massa_models::{constants::PERIODS_PER_CYCLE, prehash::Map, Address, Amount, Slot};

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
    pub fn apply_changes(&mut self, changes: PoSChanges, slot: Slot) {
        /// compute the current cycle from the given slot
        let cycle = slot.get_cycle(PERIODS_PER_CYCLE);

        // if cycle C is absent from self.cycle_history:
        // push a new empty CycleInfo at the back of self.cycle_history and set its cycle = C
        // pop_front from cycle_history until front() represents cycle C-4 or later
        // (not C-3 because we might need older endorsement draws on the limit between 2 cycles)
        if !self.cycle_history.iter().any(|info| info.cycle == cycle) {
            self.cycle_history.push_back(CycleInfo {
                cycle,
                ..Default::default()
            });
            if cycle_history.len() > 4 {
                self.cycle_history.pop_front();
            }
        }

        // extend seed_bits with changes.seed_bits
        // extend roll_counts with changes.roll_changes and remove entries for which Amount = 0
        // extend production_stats with changes.production_stats
        let current = self.cycle_history.back_mut().unwrap();
        current.rng_seed.extend(changes.seed_bits);
        current.roll_counts.extend(changes.roll_changes);
        current.roll_counts.drain_filter(|_, &mut count| count == 0);
        current.production_stats.extend(changes.production_stats);

        // extent deferred_credits with changes.deferred_credits
        // remove executed credits from the map
        self.deferred_credits.extend(changes.deferred_credits);
        self.deferred_credits
            .drain_filter(|&credit_slot, _| credit_slot < slot);

        // feed the cycle if it is complete
        // if slot S was the last of cycle C:
        // set complete=true for cycle C in the history
        // notify the PoSDrawer for cycle C+3
        if slot.last_in_cycle() {
            current.complete = true;
            self.selector
                .as_ref()
                .expect("critical: SelectorController is missing from PoSFinalState")
                .feed_cycle(current.clone());
        }
    }

    /// Retrieves the amount of rolls a given address has
    pub fn get_rolls_for(&self, addr: &Address) -> u64 {
        self.cycle_history
            .back()
            .and_then(|info| info.roll_counts.get(addr).cloned())
            .unwrap_or_default()
    }

    /// Retrives every deferred credit of the given slot
    pub fn get_deferred_credits_at(&self, slot: &Slot) -> Map<Address, Amount> {
        self.deferred_credits.get(slot).cloned().unwrap_or_default()
    }

    /// Retrives the productions statistics
    pub fn get_production_stats(&self) -> Option<Map<Address, ProductionStats>> {
        self.cycle_history
            .back()
            .map(|info| info.production_stats.clone())
    }
}
