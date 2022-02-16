// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::LedgerConfig;
use massa_hash::hash::Hash;
use massa_models::{prehash::Map, Address, Amount, Slot};
use std::collections::{hash_map, BTreeMap, VecDeque};

/// represents a structure that supports another one being applied to it
pub trait Applicable<V> {
    fn apply(&mut self, _: V);
}

/// structure defining a ledger entry
#[derive(Default, Debug, Clone)]
pub struct LedgerEntry {
    pub parallel_balance: Amount,
    pub bytecode: Vec<u8>,
    pub datastore: BTreeMap<Hash, Vec<u8>>,
}

/// LedgerEntryUpdate can be applied to a LedgerEntry
impl Applicable<LedgerEntryUpdate> for LedgerEntry {
    /// applies a LedgerEntryUpdate
    fn apply(&mut self, update: LedgerEntryUpdate) {
        update.parallel_balance.apply_to(&mut self.parallel_balance);
        update.bytecode.apply_to(&mut self.bytecode);
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
    }
}

/// represents a set/update/delete change
#[derive(Debug, Clone)]
pub enum SetUpdateOrDelete<T: Default + Applicable<V>, V: Applicable<V> + Clone> {
    /// sets a new absolute value T
    Set(T),
    /// applies an update V to an existing value
    Update(V),
    /// deletes a value
    Delete,
}

/// supports applying another SetUpdateOrDelete to self
impl<T: Default + Applicable<V>, V: Applicable<V>> Applicable<SetUpdateOrDelete<T, V>>
    for SetUpdateOrDelete<T, V>
where
    V: Clone,
{
    fn apply(&mut self, other: SetUpdateOrDelete<T, V>) {
        match other {
            // the other SetUpdateOrDelete sets a new absolute value => force it on self
            v @ SetUpdateOrDelete::Set(_) => *self = v,

            // the other SetUpdateOrDelete updates the value
            SetUpdateOrDelete::Update(u) => match self {
                // if self currently sets an absolute value, apply other to that value
                SetUpdateOrDelete::Set(cur) => cur.apply(u),

                // if self currently updates a value, apply the updates of the other to that update
                SetUpdateOrDelete::Update(cur) => cur.apply(u),

                // if self currently deletes a value,
                // create a new default value, apply other's updates to it and make self set it as an absolute new value
                SetUpdateOrDelete::Delete => {
                    let mut res = T::default();
                    res.apply(u);
                    *self = SetUpdateOrDelete::Set(res);
                }
            },

            // the other SetUpdateOrDelete deletes a value => force self to delete it as well
            v @ SetUpdateOrDelete::Delete => *self = v,
        }
    }
}

/// represents a set/delete change
#[derive(Debug, Clone)]
pub enum SetOrDelete<T: Clone> {
    /// sets a new absolute value T
    Set(T),
    /// deletes a value
    Delete,
}

/// allows applying another SetOrDelete to the current one
impl<T: Clone> Applicable<SetOrDelete<T>> for SetOrDelete<T> {
    fn apply(&mut self, other: Self) {
        *self = other;
    }
}

/// represents a set/keep change
#[derive(Debug, Clone)]
pub enum SetOrKeep<T: Clone> {
    /// sets a new absolute value T
    Set(T),
    /// keeps the existing value
    Keep,
}

/// allows applying another SetOrKeep to the current one
impl<T: Clone> Applicable<SetOrKeep<T>> for SetOrKeep<T> {
    fn apply(&mut self, other: SetOrKeep<T>) {
        if let v @ SetOrKeep::Set(..) = other {
            // update the current value only if the other SetOrKeep sets a new one
            *self = v;
        }
    }
}

impl<T: Clone> SetOrKeep<T> {
    /// applies the current SetOrKeep into a target mutable value
    pub fn apply_to(self, val: &mut T) {
        if let SetOrKeep::Set(v) = self {
            // only change the value if self is setting a new one
            *val = v;
        }
    }
}

impl<T: Clone> Default for SetOrKeep<T> {
    fn default() -> Self {
        SetOrKeep::Keep
    }
}

/// represents an update to one or more fields of a LedgerEntry
#[derive(Default, Debug, Clone)]
pub struct LedgerEntryUpdate {
    roll_count: SetOrKeep<u64>,
    parallel_balance: SetOrKeep<Amount>,
    bytecode: SetOrKeep<Vec<u8>>,
    datastore: Map<Hash, SetOrDelete<Vec<u8>>>,
}

impl Applicable<LedgerEntryUpdate> for LedgerEntryUpdate {
    /// extends the LedgerEntryUpdate with another one
    fn apply(&mut self, update: LedgerEntryUpdate) {
        self.roll_count.apply(update.roll_count);
        self.parallel_balance.apply(update.parallel_balance);
        self.bytecode.apply(update.bytecode);
        self.datastore.extend(update.datastore);
    }
}

/// represents a list of changes to ledger entries
#[derive(Default, Debug, Clone)]
pub struct LedgerChanges(pub Map<Address, SetUpdateOrDelete<LedgerEntry, LedgerEntryUpdate>>);

impl Applicable<LedgerChanges> for LedgerChanges {
    /// extends the current LedgerChanges with another one
    fn apply(&mut self, changes: LedgerChanges) {
        for (addr, change) in changes.0 {
            match self.0.entry(addr) {
                hash_map::Entry::Occupied(mut occ) => {
                    // apply incoming change if a change on this entry already exists
                    occ.get_mut().apply(change);
                }
                hash_map::Entry::Vacant(vac) => {
                    // otherwise insert the incoming change
                    vac.insert(change);
                }
            }
        }
    }
}

impl LedgerChanges {
    /// tries to return the parallel balance or gets it from a function
    ///
    /// # Returns
    ///     * Some(v) if a value is present
    ///     * None if the value is absent
    ///     * f() if the value is unknown
    ///
    /// this is used as an optimization:
    /// if the value can be deduced unambiguously from the LedgerChanges, no need to dig further
    pub fn get_parallel_balance_or_else<F: FnOnce() -> Option<Amount>>(
        &self,
        addr: &Address,
        f: F,
    ) -> Option<Amount> {
        match self.0.get(addr) {
            Some(SetUpdateOrDelete::Set(v)) => Some(v.parallel_balance),
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                parallel_balance, ..
            })) => match parallel_balance {
                SetOrKeep::Set(v) => Some(*v),
                SetOrKeep::Keep => f(),
            },
            Some(SetUpdateOrDelete::Delete) => None,
            None => f(),
        }
    }

    /// tries to return the bytecode or gets it from a function
    ///
    /// # Returns
    ///     * Some(v) if a value is present
    ///     * None if the value is absent
    ///     * f() if the value is unknown
    ///
    /// this is used as an optimization:
    /// if the value can be deduced unambiguously from the LedgerChanges, no need to dig further
    pub fn get_bytecode_or_else<F: FnOnce() -> Option<Vec<u8>>>(
        &self,
        addr: &Address,
        f: F,
    ) -> Option<Vec<u8>> {
        match self.0.get(addr) {
            Some(SetUpdateOrDelete::Set(v)) => Some(v.bytecode.clone()),
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { bytecode, .. })) => match bytecode {
                SetOrKeep::Set(v) => Some(v.clone()),
                SetOrKeep::Keep => f(),
            },
            Some(SetUpdateOrDelete::Delete) => None,
            None => f(),
        }
    }

    /// tries to return whether an entry exists or gets it from a function
    ///
    /// # Returns
    ///     * true if a entry is present
    ///     * false if the entry is absent
    ///     * f() if the existence of the value is unknown
    ///
    /// this is used as an optimization:
    /// if the value can be deduced unambiguously from the LedgerChanges, no need to dig further
    pub fn entry_exists_or_else<F: FnOnce() -> bool>(&self, addr: &Address, f: F) -> bool {
        match self.0.get(addr) {
            Some(SetUpdateOrDelete::Set(_)) => true,
            Some(SetUpdateOrDelete::Update(_)) => true,
            Some(SetUpdateOrDelete::Delete) => false,
            None => f(),
        }
    }

    /// set the parallel balance of an address
    pub fn set_parallel_balance(&mut self, addr: Address, balance: Amount) {
        match self.0.entry(addr) {
            hash_map::Entry::Occupied(mut occ) => {
                match occ.get_mut() {
                    SetUpdateOrDelete::Set(v) => {
                        // we currently set the absolute value of the entry
                        // so we need to update the parallel_balance of that value
                        v.parallel_balance = balance;
                    }
                    SetUpdateOrDelete::Update(u) => {
                        // we currently update the value of the entry
                        // so we need to set the parallel_balance for that update
                        u.parallel_balance = SetOrKeep::Set(balance);
                    }
                    d @ SetUpdateOrDelete::Delete => {
                        // we currently delete the entry
                        // so we need to create a default one with the target balance
                        *d = SetUpdateOrDelete::Set(LedgerEntry {
                            parallel_balance: balance,
                            ..Default::default()
                        });
                    }
                }
            }
            hash_map::Entry::Vacant(vac) => {
                // we currently aren't changing anything on that entry
                // so we need to create an update with the target balance
                vac.insert(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    parallel_balance: SetOrKeep::Set(balance),
                    ..Default::default()
                }));
            }
        }
    }

    /// set the parallel balance of an address
    pub fn set_bytecode(&mut self, addr: Address, bytecode: Vec<u8>) {
        match self.0.entry(addr) {
            hash_map::Entry::Occupied(mut occ) => {
                match occ.get_mut() {
                    SetUpdateOrDelete::Set(v) => {
                        // we currently set the absolute value of the entry
                        // so we need to update the bytecode of that value
                        v.bytecode = bytecode;
                    }
                    SetUpdateOrDelete::Update(u) => {
                        // we currently update the value of the entry
                        // so we need to set the bytecode for that update
                        u.bytecode = SetOrKeep::Set(bytecode);
                    }
                    d @ SetUpdateOrDelete::Delete => {
                        // we currently delete the entry
                        // so we need to create a default one with the target bytecode
                        *d = SetUpdateOrDelete::Set(LedgerEntry {
                            bytecode,
                            ..Default::default()
                        });
                    }
                }
            }
            hash_map::Entry::Vacant(vac) => {
                // we currently aren't changing anything on that entry
                // so we need to create an update with the target bytecode
                vac.insert(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    bytecode: SetOrKeep::Set(bytecode),
                    ..Default::default()
                }));
            }
        }
    }

    /// tries to return a data entry
    ///
    /// # Returns
    ///     * Some(v) if a value is present
    ///     * None if the value is absent
    ///     * f() if the value is unknown
    ///
    /// this is used as an optimization:
    /// if the value can be deduced unambiguously from the LedgerChanges, no need to dig further
    pub fn get_data_entry_or_else<F: FnOnce() -> Option<Vec<u8>>>(
        &self,
        addr: &Address,
        key: &Hash,
        f: F,
    ) -> Option<Vec<u8>> {
        match self.0.get(addr) {
            Some(SetUpdateOrDelete::Set(v)) => v.datastore.get(key).cloned(),
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { datastore, .. })) => {
                match datastore.get(key) {
                    Some(SetOrDelete::Set(v)) => Some(v.clone()),
                    Some(SetOrDelete::Delete) => None,
                    None => f(),
                }
            }
            Some(SetUpdateOrDelete::Delete) => None,
            None => f(),
        }
    }

    /// tries to return whether a data entry exists
    ///
    /// # Returns
    ///     * true if it does
    ///     * false if it does not
    ///     * f() if its existance is unknown
    ///
    /// this is used as an optimization:
    /// if the value can be deduced unambiguously from the LedgerChanges, no need to dig further
    pub fn has_data_entry_or_else<F: FnOnce() -> bool>(
        &self,
        addr: &Address,
        key: &Hash,
        f: F,
    ) -> bool {
        match self.0.get(addr) {
            Some(SetUpdateOrDelete::Set(v)) => v.datastore.contains_key(key),
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { datastore, .. })) => {
                match datastore.get(key) {
                    Some(SetOrDelete::Set(_)) => true,
                    Some(SetOrDelete::Delete) => false,
                    None => f(),
                }
            }
            Some(SetUpdateOrDelete::Delete) => false,
            None => f(),
        }
    }

    /// set a datastore entry for an address
    pub fn set_data_entry(&mut self, addr: Address, key: Hash, data: Vec<u8>) {
        match self.0.entry(addr) {
            hash_map::Entry::Occupied(mut occ) => {
                match occ.get_mut() {
                    SetUpdateOrDelete::Set(v) => {
                        // we currently set the absolute value of the entry
                        // so we need to update the data of that value
                        v.datastore.insert(key, data);
                    }
                    SetUpdateOrDelete::Update(u) => {
                        // we currently update the value of the entry
                        // so we need to set the data for that update
                        u.datastore.insert(key, SetOrDelete::Set(data));
                    }
                    d @ SetUpdateOrDelete::Delete => {
                        // we currently delete the entry
                        // so we need to create a default one with the target data
                        *d = SetUpdateOrDelete::Set(LedgerEntry {
                            datastore: vec![(key, data)].into_iter().collect(),
                            ..Default::default()
                        });
                    }
                }
            }
            hash_map::Entry::Vacant(vac) => {
                // we currently aren't changing anything on that entry
                // so we need to create an update with the target data
                vac.insert(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    datastore: vec![(key, SetOrDelete::Set(data))].into_iter().collect(),
                    ..Default::default()
                }));
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
    sorted_ledger: BTreeMap<Address, LedgerEntry>,
    /// history of recent final ledger changes
    /// front = oldest, back = newest
    changes_history: VecDeque<(Slot, LedgerChanges)>,
}

impl Applicable<LedgerChanges> for FinalLedger {
    /// merges LedgerChanges to the final ledger
    fn apply(&mut self, changes: LedgerChanges) {
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
                        .apply(entry_update);
                }
                SetUpdateOrDelete::Delete => {
                    // deletes an entry, if it exists
                    self.sorted_ledger.remove(&addr);
                }
            }
        }
    }
}

impl FinalLedger {
    /// settles a slot and saves the corresponding ledger changes to history
    pub fn settle_slot(&mut self, slot: Slot, changes: LedgerChanges) {
        // apply changes
        self.apply(changes.clone());

        // update the slot
        self.slot = slot;

        // update and prune changes history
        self.changes_history.push_back((slot, changes));
        while self.changes_history.len() > self.config.final_history_length {
            self.changes_history.pop_front();
        }
    }

    /// gets the parallel balance of an entry
    pub fn get_parallel_balance(&self, addr: &Address) -> Option<Amount> {
        self.sorted_ledger.get(addr).map(|v| v.parallel_balance)
    }

    /// gets a copy of the bytecode of an entry
    pub fn get_bytecode(&self, addr: &Address) -> Option<Vec<u8>> {
        self.sorted_ledger.get(addr).map(|v| v.bytecode.clone())
    }

    /// checks if an entry exists
    pub fn entry_exists(&self, addr: &Address) -> bool {
        self.sorted_ledger.contains_key(addr)
    }

    /// gets a copy of a data entry
    pub fn get_data_entry(&self, addr: &Address, key: &Hash) -> Option<Vec<u8>> {
        self.sorted_ledger
            .get(addr)
            .and_then(|v| v.datastore.get(key).cloned())
    }

    /// checks whether a data entry exists
    pub fn has_data_entry(&self, addr: &Address, key: &Hash) -> bool {
        self.sorted_ledger
            .get(addr)
            .map_or(false, |v| v.datastore.contains_key(key))
    }
}
