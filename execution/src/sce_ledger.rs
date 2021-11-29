use massa_hash::hash::Hash;
use std::sync::{Arc, Mutex};

use crate::ExecutionError;
use models::ModelsError;
use models::{address::AddressHashMap, hhasher::HHashMap, Address, Amount, AMOUNT_ZERO};
use wasmer::Module;

/// an entry in the SCE ledger
#[derive(Debug, Clone, Default)]
pub struct SCELedgerEntry {
    pub balance: Amount,
    pub opt_module: Option<Module>,
    pub data: HHashMap<Hash, Vec<u8>>,
}

impl SCELedgerEntry {
    /// applies an entry update to self
    pub fn apply_entry_update(&mut self, update: &SCELedgerEntryUpdate) {
        // balance
        if let Some(new_balance) = update.update_balance {
            self.balance = new_balance;
        }

        // module
        if let Some(opt_module) = &update.update_opt_module {
            self.opt_module = opt_module.clone();
        }

        // data
        for (data_key, data_update) in update.update_data.iter() {
            match data_update {
                Some(new_data) => {
                    self.data.insert(*data_key, new_data.clone());
                }
                None => {
                    self.data.remove(data_key);
                }
            }
        }
    }
}

// optional updates to be applied to a ledger entry
#[derive(Debug, Clone)]
pub struct SCELedgerEntryUpdate {
    pub update_balance: Option<Amount>,
    pub update_opt_module: Option<Option<Module>>,
    pub update_data: HHashMap<Hash, Option<Vec<u8>>>, // None for row deletion
}

impl SCELedgerEntryUpdate {
    /// apply another SCELedgerEntryUpdate to self
    pub fn apply_entry_update(&mut self, other: &SCELedgerEntryUpdate) {
        // balance
        if let Some(new_balance) = other.update_balance {
            self.update_balance = Some(new_balance);
        }

        // module
        if let Some(new_opt_module) = &other.update_opt_module {
            self.update_opt_module = Some(new_opt_module.clone());
        }

        // data
        self.update_data.extend(other.update_data.clone());
    }
}

#[derive(Debug, Clone)]
pub enum SCELedgerChange {
    // delete an entry
    Delete,

    // sets an entry to an absolute value
    Set(SCELedgerEntry),

    // updates an entry
    Update(SCELedgerEntryUpdate),
}

impl SCELedgerChange {
    /// applies another SCELedgerChange to the current one
    pub fn apply_change(&mut self, other: &SCELedgerChange) {
        let new_val = match (&self, other) {
            // other deletes the entry
            (_, SCELedgerChange::Delete) => {
                // make self delete as well
                SCELedgerChange::Delete
            }

            // other sets an absolute entry
            (_, new_set @ SCELedgerChange::Set(_)) => {
                // make self set the same absolute entry
                new_set.clone()
            }

            // self deletes, other updates
            (SCELedgerChange::Delete, SCELedgerChange::Update(other_entry_update)) => {
                // prepare a default entry
                let mut res_entry = SCELedgerEntry::default();
                // apply other's updates to res_entry
                res_entry.apply_entry_update(other_entry_update);
                // make self set to res_entry
                SCELedgerChange::Set(res_entry)
            }

            // self sets, other updates
            (SCELedgerChange::Set(cur_entry), SCELedgerChange::Update(other_entry_update)) => {
                // apply other's updates to cur_entry
                // TODO avoid clone, act directly on mutable cur_entry
                let mut res_entry = cur_entry.clone();
                res_entry.apply_entry_update(other_entry_update);
                SCELedgerChange::Set(res_entry)
            }

            // self updates, other updates
            (
                SCELedgerChange::Update(cur_entry_update),
                SCELedgerChange::Update(other_entry_update),
            ) => {
                // try to apply other's updates to self's updates
                // TODO avoid clone, act directly on mutable cur_entry_update
                let mut res_update = cur_entry_update.clone();
                res_update.apply_entry_update(other_entry_update);
                SCELedgerChange::Update(res_update)
            }
        };
        *self = new_val;
    }
}

/// SCE ledger
#[derive(Debug, Clone, Default)]
pub struct SCELedger(AddressHashMap<SCELedgerEntry>);

/// list of ledger changes (deletions, resets, updates)
#[derive(Debug, Clone, Default)]
pub struct SCELedgerChanges(pub AddressHashMap<SCELedgerChange>);

impl SCELedgerChanges {
    /// extends the current SCELedgerChanges with another
    pub fn apply_changes(&mut self, changes: &SCELedgerChanges) {
        for (addr, change) in changes.0.iter() {
            self.apply_change(*addr, change);
        }
    }

    /// appliees a single change to self
    pub fn apply_change(&mut self, addr: Address, change: &SCELedgerChange) {
        self.0
            .entry(addr)
            .and_modify(|cur_c| cur_c.apply_change(change))
            .or_insert_with(|| change.clone());
    }
}

impl SCELedger {
    /// applies ledger changes to ledger
    pub fn apply_changes(&mut self, changes: &SCELedgerChanges) {
        for (addr, change) in changes.0.iter() {
            match change {
                // delete entry
                SCELedgerChange::Delete => {
                    self.0.remove(addr);
                }

                // set entry to absolute value
                SCELedgerChange::Set(new_entry) => {
                    self.0.insert(*addr, new_entry.clone());
                }

                // update entry
                SCELedgerChange::Update(update) => {
                    // insert default if absent
                    self.0
                        .entry(*addr)
                        .or_insert_with(|| SCELedgerEntry::default())
                        .apply_entry_update(update);
                }
            }
        }
    }
}

/// represents an execution step from the point of view of the SCE ledger
///   a reference to the final ledger, as well as an accumulator of existing ledger changes allow computing the input SCE ledger state
///   caused_changes lists the additional changes caused by the step
#[derive(Debug, Clone)]
pub struct SCELedgerStep {
    pub final_ledger: Arc<Mutex<SCELedger>>,
    pub cumulative_history_changes: SCELedgerChanges,
    pub caused_changes: SCELedgerChanges,
}

impl SCELedgerStep {
    /// gets the balance of an SCE ledger entry
    pub fn get_balance(&self, addr: &Address) -> Amount {
        // check if caused_changes or cumulative_history_changes have an update on this
        for changes in [&self.caused_changes, &self.cumulative_history_changes] {
            match changes.0.get(addr) {
                Some(SCELedgerChange::Delete) => return AMOUNT_ZERO,
                Some(SCELedgerChange::Set(new_entry)) => return new_entry.balance,
                Some(SCELedgerChange::Update(update)) => {
                    if let Some(updated_balance) = update.update_balance {
                        return updated_balance;
                    }
                }
                None => {}
            }
        }

        // check if the final ledger has the info
        {
            let ledger_guard = self.final_ledger.lock().unwrap();
            if let Some(entry) = (*ledger_guard).0.get(addr) {
                return entry.balance;
            }
        }

        // otherwise, just return zero
        AMOUNT_ZERO
    }

    /// sets the balance of an address
    pub fn set_balance(&mut self, addr: Address, balance: Amount) {
        let update = SCELedgerEntryUpdate {
            update_balance: Some(balance),
            update_opt_module: Default::default(),
            update_data: Default::default(),
        };
        self.caused_changes
            .apply_change(addr, &SCELedgerChange::Update(update));
    }

    /// tries to increase/decrease the balance of an address
    /// does not change anything on failure
    pub fn set_balance_delta(
        &mut self,
        addr: Address,
        amount: Amount,
        positive: bool,
    ) -> Result<(), ExecutionError> {
        let mut balance = self.get_balance(&addr);
        if positive {
            balance = balance
                .checked_add(amount)
                .ok_or(ModelsError::CheckedOperationError(
                    "balance overflow".into(),
                ))?;
        } else {
            balance = balance
                .checked_sub(amount)
                .ok_or(ModelsError::CheckedOperationError(
                    "balance underflow".into(),
                ))?;
        }
        self.set_balance(addr, balance);
        Ok(())
    }

    /// gets the module of an SCE ledger entry
    ///  returns None if the entry was not found or has no module
    pub fn get_module(&self, addr: &Address) -> Option<Module> {
        // check if caused_changes or cumulative_history_changes have an update on this
        for changes in [&self.caused_changes, &self.cumulative_history_changes] {
            match changes.0.get(addr) {
                Some(SCELedgerChange::Delete) => return None,
                Some(SCELedgerChange::Set(new_entry)) => return new_entry.opt_module.clone(),
                Some(SCELedgerChange::Update(update)) => {
                    if let Some(updates_opt_module) = &update.update_opt_module {
                        return updates_opt_module.clone();
                    }
                }
                None => {}
            }
        }

        // check if the final ledger has the info
        {
            let ledger_guard = self.final_ledger.lock().unwrap();
            if let Some(entry) = (*ledger_guard).0.get(addr) {
                return entry.opt_module.clone();
            }
        }

        // otherwise, return None
        None
    }

    /// returns a data entry
    ///   None if address not found or entry nto found in addr's data
    pub fn get_data_entry(&self, addr: &Address, key: &Hash) -> Option<Vec<u8>> {
        // check if caused_changes or cumulative_history_changes have an update on this
        for changes in [&self.caused_changes, &self.cumulative_history_changes] {
            match changes.0.get(addr) {
                Some(SCELedgerChange::Delete) => return None,
                Some(SCELedgerChange::Set(new_entry)) => return new_entry.data.get(key).cloned(),
                Some(SCELedgerChange::Update(update)) => {
                    match update.update_data.get(key) {
                        None => {}                 // no updates
                        Some(None) => return None, // data entrt deleted,
                        Some(Some(updated_data)) => return Some(updated_data.clone()),
                    }
                }
                None => {}
            }
        }

        // check if the final ledger has the info
        {
            let ledger_guard = self.final_ledger.lock().unwrap();
            if let Some(entry) = (*ledger_guard).0.get(addr) {
                return entry.data.get(key).cloned();
            }
        }

        // otherwise, return None
        None
    }

    /// sets data entry
    pub fn set_data_entry(&mut self, addr: Address, key: Hash, value: Vec<u8>) {
        let update = SCELedgerEntryUpdate {
            update_balance: Default::default(),
            update_opt_module: Default::default(),
            update_data: [(key, Some(value))].into_iter().collect(),
        };
        self.caused_changes
            .apply_change(addr, &SCELedgerChange::Update(update));
    }
}
