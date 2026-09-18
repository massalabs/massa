use crate::{DeferredCredits, ProductionStats};
use bitvec::prelude::*;
use massa_models::{address::Address, prehash::PreHashMap};
use serde::{Deserialize, Serialize};

/// Recap of all PoS changes
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PoSChanges {
    /// extra block seed bits added
    pub seed_bits: BitVec<u8>,

    /// new roll counts for addresses (can be 0 to remove the address from the registry)
    pub roll_changes: PreHashMap<Address, u64>,

    /// updated production statistics
    pub production_stats: PreHashMap<Address, ProductionStats>,

    /// set deferred credits indexed by target slot (can be set to 0 to cancel some, in case of slash)
    /// ordered structure to ensure slot iteration order is deterministic
    pub deferred_credits: DeferredCredits,
}

impl PoSChanges {
    /// Check if changes are empty
    pub fn is_empty(&self) -> bool {
        self.seed_bits.is_empty()
            && self.roll_changes.is_empty()
            && self.production_stats.is_empty()
            && self.deferred_credits.credits.is_empty()
    }

    /// Extends the current `PosChanges` with another one
    pub fn extend(&mut self, other: PoSChanges) {
        // extend seed bits
        self.seed_bits.extend(other.seed_bits);

        // extend roll changes
        self.roll_changes.extend(other.roll_changes);

        // extend production stats
        for (other_addr, other_stats) in other.production_stats {
            self.production_stats
                .entry(other_addr)
                .or_default()
                .extend(&other_stats);
        }

        // extend deferred credits
        self.deferred_credits.extend(other.deferred_credits);
    }
}
