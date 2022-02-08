use std::sync::{Arc, RwLock};

use massa_ledger::{FinalLedger, LedgerChanges, LedgerEntryUpdate, SetOrKeep, SetUpdateOrDelete};
use massa_models::{Address, Amount};

/// represents a speculative ledger state combining
/// data from the final ledger, previous speculative changes,
/// and accumulated changes since the construction of the object
pub struct SpeculativeLedger {
    /// accumulation of previous changes
    previous_changes: LedgerChanges,

    /// list of added changes
    pub added_changes: LedgerChanges,
}

impl SpeculativeLedger {
    /// creates a new SpeculativeLedger
    pub fn new(previous_changes: LedgerChanges) -> Self {
        SpeculativeLedger {
            previous_changes,
            added_changes: Default::default(),
        }
    }

    /// gets the sequential balance of an address
    pub fn get_sequential_balance(
        &self,
        addr: &Address,
        final_ledger: &FinalLedger,
    ) -> Option<Amount> {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.get_sequential_balance_or_else(addr, || {
            self.previous_changes
                .get_sequential_balance_or_else(addr, || final_ledger.get_sequential_balance(addr))
        })
    }
}
