// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::ledger_entry::LedgerEntry;
use crate::types::{Applicable, SetOrDelete, SetOrKeep, SetUpdateOrDelete};
use massa_hash::hash::Hash;
use massa_models::{prehash::Map, Address, Amount};
use std::collections::hash_map;

/// represents an update to one or more fields of a LedgerEntry
#[derive(Default, Debug, Clone)]
pub struct LedgerEntryUpdate {
    pub roll_count: SetOrKeep<u64>,
    pub parallel_balance: SetOrKeep<Amount>,
    pub bytecode: SetOrKeep<Vec<u8>>,
    pub datastore: Map<Hash, SetOrDelete<Vec<u8>>>,
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
    /// get an item
    pub fn get(
        &self,
        addr: &Address,
    ) -> Option<&SetUpdateOrDelete<LedgerEntry, LedgerEntryUpdate>> {
        self.0.get(addr)
    }

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
