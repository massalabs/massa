use massa_hash::hash::Hash;
use models::ModelsError;
use models::{address::AddressHashMap, hhasher::HHashMap, Address, Amount, AMOUNT_ZERO};
use wasmer::Module;

use crate::ExecutionError;

#[derive(Debug, Clone, Default)]
pub struct SCELedgerEntry {
    pub balance: Amount,
    pub opt_module: Option<Module>,
    pub data: HHashMap<Hash, Vec<u8>>,
}

/// represents an SCE ledger
#[derive(Debug, Clone, Default)]
pub struct SCELedger(AddressHashMap<SCELedgerEntry>);

#[derive(Debug, Clone, Default)]
pub struct SCELedgerChanges(Vec<SCELedgerChange>);

/// defines a change caused on the ledger
#[derive(Debug, Clone)]
pub enum SCELedgerChange {
    // entry added to the ledger
    AddEntry {
        balance: Amount,
        module: Option<Module>,
        data: HHashMap<Hash, Vec<u8>>
    },

    // entry removed from the ledger
    RemoveEntry,

    // balance changed
    ChangeBalance {
        delta: Amount,
        positive: bool
    },

    // module changed
    ChangeModule {
        module: Option<Module>  // None to remove
    },

    // data changed
    DataChange {
        changes: HHashMap<Hash, Option<Vec<u8>>>  // None to delete
    }
}

impl SCELedgerChanges {

    /// extends/overwrites the current SCELedgerChanges with the changes from another
    pub fn extend(&mut self, changes: &SCELedgerChanges) {
        for (addr, change) in changes.0.iter() {
            self.insert_change(*addr, change);
        }
    }

    /// extends the current SCELedgerChanges with a single change
    pub fn insert_change(&mut self, addr: Address, change: &SCELedgerChange) {

    }


}

impl SCELedger {
    /// returns the balance of an address
    /// zero if the address is absent from the ledger
    pub fn get_balance(&mut self, apply_changes: Vec<LedgerChange>, addr: &Address) -> Amount {
        let mut balance = self.0.get(addr).map_or(AMOUNT_ZERO, |entry| entry.balance);
        for change in apply_changes.iter() {
            match change {
                SCELedgerChange::AddEntry { addr, ..} => if addr,
                _ => {}
            }
        }
    }

    /// applies a balance change by delta
    /// no effect in case of failure
    pub fn change_balance(
        &mut self,
        addr: &Address,
        delta: Amount,
        positive: bool,
    ) -> Result<(), ExecutionError> {
        // ignore if delta is zero
        if delta.is_zero() {
            return Ok(());
        }

        // get current balance
        let mut balance = self.get_balance(addr);

        // try updating the balance
        balance = if positive {
            balance
                .checked_add(delta)
                .ok_or(ModelsError::CheckedOperationError(
                    "balance overflow".into(),
                ))?
        } else {
            balance
                .checked_sub(delta)
                .ok_or(ModelsError::CheckedOperationError(
                    "balance underflow".into(),
                ))?
        };

        // register new balance
        self.0

        Ok(())
    }

    /// merge all changes into the final_ledger
    pub fn merge_changes_into_final(&mut self) {
        let mut final_ledger_guard = self.final_ledger.lock().unwrap();
        for (addr, entry) in self.ledger_changes.drain() {
            (*final_ledger_guard).insert(addr, entry);
        }
    }
}
