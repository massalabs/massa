//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file provides structures representing changes to the final state

use massa_async_pool::AsyncPoolChanges;
use massa_ledger::LedgerChanges;

/// represents changes that can be applied to the execution state
#[derive(Default, Debug, Clone)]
pub struct StateChanges {
    /// ledger changes
    pub ledger_changes: LedgerChanges,
    /// asynchronous pool changes
    pub async_pool_changes: AsyncPoolChanges,
}

impl StateChanges {
    /// extends the current `StateChanges` with another one
    pub fn apply(&mut self, changes: StateChanges) {
        use massa_ledger::Applicable;
        self.ledger_changes.apply(changes.ledger_changes);
        self.async_pool_changes.extend(changes.async_pool_changes);
    }
}
