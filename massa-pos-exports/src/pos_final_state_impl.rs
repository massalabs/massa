use massa_models::{constants::PERIODS_PER_CYCLE, prehash::Map, Address, Amount, Slot};

use crate::{CycleInfo, PoSChanges, PoSFinalState, ProductionStats};

impl PoSFinalState {
    /// Finalizes changes at a slot S (cycle C):
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
    pub fn apply_changes(&mut self, changes: PoSChanges, slot: Slot) {
        let cycle = slot.get_cycle(PERIODS_PER_CYCLE);
        // if cycle not in history push a new one and pop front
        if !self.cycle_history.iter().any(|info| info.cycle == cycle) {
            self.cycle_history.push_back(CycleInfo {
                cycle,
                ..Default::default()
            });
            self.cycle_history.pop_front();
        }
        // can't fail because of previous check
        let current = self.cycle_history.back_mut().unwrap();
        current.rng_seed.extend(changes.seed_bits);
        current.roll_counts.extend(changes.roll_changes);
        current.roll_counts.drain_filter(|_, &mut count| count == 0);
        current.production_stats.extend(changes.production_stats);
        self.deferred_credits = changes.deferred_credits;
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
