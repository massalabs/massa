use massa_models::{Address, Slot};

use crate::{PoSChanges, PoSFinalState};

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
    pub fn apply_changes(&mut self, _changes: &PoSChanges, _slot: Slot) {}

    /// TODO
    pub fn get_rolls_for(&self, addr: &Address) -> Option<u64> {
        self.cycle_history
            .back()
            .map(|info| info.roll_counts.get(addr).cloned())
            .flatten()
    }
}
