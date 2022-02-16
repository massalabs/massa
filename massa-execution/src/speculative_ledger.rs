use std::sync::{Arc, RwLock};

use massa_ledger::{FinalLedger, LedgerChanges, LedgerEntryUpdate, SetOrKeep, SetUpdateOrDelete};
use massa_models::{Address, Amount};

/// represents a speculative ledger state combining
/// data from the final ledger, previous speculative changes,
/// and accumulated changes since the construction of the object
pub struct SpeculativeLedger {
    /// final ledger
    final_ledger: Arc<RwLock<FinalLedger>>,

    /// accumulation of previous changes
    previous_changes: LedgerChanges,

    /// list of added changes
    added_changes: LedgerChanges,
}

impl SpeculativeLedger {
    /// creates a new SpeculativeLedger
    pub fn new(final_ledger: Arc<RwLock<FinalLedger>>, previous_changes: LedgerChanges) -> Self {
        SpeculativeLedger {
            final_ledger,
            previous_changes,
            added_changes: Default::default(),
        }
    }

    /// takes a snapshot (clone) of the added changes
    pub fn get_snapshot(&self) -> LedgerChanges {
        self.added_changes.clone()
    }

    /// resets to a snapshot of added ledger changes
    pub fn reset_to_snapshot(&mut self, snapshot: LedgerChanges) {
        self.added_changes = snapshot;
    }

    /// consumes Self to get added changes
    pub fn into_added_changes(self) -> LedgerChanges {
        self.added_changes
    }

    /// gets the parallel balance of an address
    pub fn get_parallel_balance(
        &self,
        addr: &Address,
        final_ledger: &FinalLedger,
    ) -> Option<Amount> {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.get_parallel_balance_or_else(addr, || {
            self.previous_changes
                .get_parallel_balance_or_else(addr, || final_ledger.get_parallel_balance(addr))
        })
    }
}
