use massa_hash::hash::Hash;
use massa_models::{prehash::Map, Address, Amount, Slot};
use std::collections::{hash_map, BTreeMap, HashMap, VecDeque};

use crate::LedgerConfig;

#[derive(Default, Debug, Clone)]
pub struct LedgerEntry {
    pub roll_count: u64,
    pub sequential_balance: Amount,
    pub parallel_balance: Amount,
    pub bytecode: Vec<u8>,
    pub datastore: BTreeMap<Hash, Vec<u8>>,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl LedgerEntry {
    /// applies a LedgerEntryUpdate
    pub fn apply_update(&mut self, update: LedgerEntryUpdate) {
        if let SetOrKeep::Set(v) = update.roll_count {
            self.roll_count = v;
        }
        if let SetOrKeep::Set(v) = update.sequential_balance {
            self.sequential_balance = v;
        }
        if let SetOrKeep::Set(v) = update.parallel_balance {
            self.parallel_balance = v;
        }
        if let SetOrKeep::Set(v) = update.bytecode {
            self.bytecode = v;
        }
        for (key, value_update) in update.datastore {
            match value_update {
                SetOrDelete::Set(v) => {
                    self.datastore.insert(key, v);
                }
                SetOrDelete::Delete => {
                    self.datastore.remove(&key);
                }
            }
        }
        for (key, value_update) in update.files {
            match value_update {
                SetOrDelete::Set(v) => {
                    self.files.insert(key, v);
                }
                SetOrDelete::Delete => {
                    self.files.remove(&key);
                }
            }
        }
    }
}

/// represents a set/update/delete change
#[derive(Debug, Clone)]
pub enum SetUpdateOrDelete<T, V> {
    /// sets a new absolute value T
    Set(T),
    /// applies an update V to an existing value
    Update(V),
    /// deletes a value
    Delete,
}

/// represents a set/delete change
#[derive(Debug, Clone)]
pub enum SetOrDelete<T> {
    /// sets a new absolute value T
    Set(T),
    /// deletes a value
    Delete,
}

/// represents a set/keep change
#[derive(Debug, Clone)]
pub enum SetOrKeep<T> {
    /// sets a new absolute value T
    Set(T),
    /// keeps the existing value
    Keep,
}

impl<T> Default for SetOrKeep<T> {
    fn default() -> Self {
        SetOrKeep::Keep
    }
}

#[derive(Default, Debug, Clone)]
pub struct LedgerEntryUpdate {
    roll_count: SetOrKeep<u64>,
    sequential_balance: SetOrKeep<Amount>,
    parallel_balance: SetOrKeep<Amount>,
    bytecode: SetOrKeep<Vec<u8>>,
    datastore: Map<Hash, SetOrDelete<Vec<u8>>>,
    files: HashMap<String, SetOrDelete<Vec<u8>>>,
}

impl LedgerEntryUpdate {
    /// extends the LedgerEntryUpdate with another one
    pub fn apply_update(&mut self, update: LedgerEntryUpdate) {
        if let v @ SetOrKeep::Set(..) = update.roll_count {
            self.roll_count = v;
        }
        if let v @ SetOrKeep::Set(..) = update.sequential_balance {
            self.sequential_balance = v;
        }
        if let v @ SetOrKeep::Set(..) = update.parallel_balance {
            self.parallel_balance = v;
        }
        if let v @ SetOrKeep::Set(..) = update.bytecode {
            self.bytecode = v;
        }
        self.datastore.extend(update.datastore);
        self.files.extend(update.files);
    }
}

/// represents a list of changes to ledger entries
#[derive(Default, Debug, Clone)]
pub struct LedgerChanges(Map<Address, SetUpdateOrDelete<LedgerEntry, LedgerEntryUpdate>>);

impl LedgerChanges {
    /// extends teh current LedgerChanges with another one
    pub fn apply_changes(&mut self, changes: LedgerChanges) {
        // iterate over all incoming changes
        for (addr, change) in changes.0 {
            match change {
                SetUpdateOrDelete::Set(new_entry) => {
                    // sets an entry to an absolute value, overriding any previous change
                    self.0.insert(addr, SetUpdateOrDelete::Set(new_entry));
                }
                SetUpdateOrDelete::Update(entry_update) => match self.0.entry(addr) {
                    // applies incoming updates to an entry
                    hash_map::Entry::Occupied(mut occ) => match occ.get_mut() {
                        // the entry was already being changed
                        SetUpdateOrDelete::Set(cur) => {
                            // the entry was already being set to an absolute value
                            // apply the incoming updates to that absolute value
                            cur.apply_update(entry_update);
                        }
                        SetUpdateOrDelete::Update(cur) => {
                            // the entry was already being updated
                            // extend the existing update with the incoming ones
                            cur.apply_update(entry_update);
                        }
                        cur @ SetUpdateOrDelete::Delete => {
                            // the entry was being deleted
                            // set the entry to a default absolute value to which the incoming updates are applied
                            let mut res_entry = LedgerEntry::default();
                            res_entry.apply_update(entry_update);
                            *cur = SetUpdateOrDelete::Set(res_entry);
                        }
                    },
                    hash_map::Entry::Vacant(vac) => {
                        // the entry was not being changesd: simply add the incoming update
                        vac.insert(SetUpdateOrDelete::Update(entry_update));
                    }
                },
                SetUpdateOrDelete::Delete => {
                    // deletes an entry, overriding any previous change
                    self.0.insert(addr, SetUpdateOrDelete::Delete);
                }
            }
        }
    }
}

/// represents a final ledger
pub struct FinalLedger {
    /// ledger config
    config: LedgerConfig,
    /// slot at which the final ledger is computed
    pub slot: Slot,
    /// sorted ledger tree
    /// TODO put it on the hard drive as it can reach 1TB
    pub sorted_ledger: BTreeMap<Address, LedgerEntry>,
    /// history of recent final ledger changes
    /// front = oldest, back = newest
    changes_history: VecDeque<(Slot, LedgerChanges)>,
}

impl FinalLedger {
    /// applies LedgerChanges to the final ledger
    pub fn apply_changes(&mut self, slot: Slot, changes: LedgerChanges) {
        // if the slot is in the past: ignore
        if slot <= self.slot {
            return;
        }

        // update the slot
        self.slot = slot;

        // update and prune changes history
        self.changes_history.push_back((slot, changes.clone()));
        while self.changes_history.len() > self.config.final_history_length {
            self.changes_history.pop_front();
        }

        // for all incoming changes
        for (addr, change) in changes.0 {
            match change {
                SetUpdateOrDelete::Set(new_entry) => {
                    // inserts/overwrites the entry with an incoming absolute value
                    self.sorted_ledger.insert(addr, new_entry);
                }
                SetUpdateOrDelete::Update(entry_update) => {
                    // applies updates to an entry
                    // if the entry does not exist, inserts a default one and applies the updates to it
                    self.sorted_ledger
                        .entry(addr)
                        .or_insert_with(|| Default::default())
                        .apply_update(entry_update);
                }
                SetUpdateOrDelete::Delete => {
                    // deletes an entry, if it exists
                    self.sorted_ledger.remove(&addr);
                }
            }
        }
    }
}

/*
    TODO how to evaluate the storage costs of a ledger change ?
*/
