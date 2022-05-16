// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file provides structures representing changes to ledger entries

use crate::ledger_entry::LedgerEntry;
use crate::types::{Applicable, SetOrDelete, SetOrKeep, SetUpdateOrDelete};
use massa_hash::Hash;
use massa_models::{prehash::Map, Address, Amount};
use std::collections::hash_map;

/// represents an update to one or more fields of a `LedgerEntry`
#[derive(Default, Debug, Clone)]
pub struct LedgerEntryUpdate {
    /// change the parallel balance
    pub parallel_balance: SetOrKeep<Amount>,
    /// change the executable bytecode
    pub bytecode: SetOrKeep<Vec<u8>>,
    // change datastore entries
    pub datastore: Map<Hash, SetOrDelete<Vec<u8>>>,
}

impl Applicable<LedgerEntryUpdate> for LedgerEntryUpdate {
    /// extends the `LedgerEntryUpdate` with another one
    fn apply(&mut self, update: LedgerEntryUpdate) {
        self.parallel_balance.apply(update.parallel_balance);
        self.bytecode.apply(update.bytecode);
        self.datastore.extend(update.datastore);
    }
}

/// represents a list of changes to multiple ledger entries
#[derive(Default, Debug, Clone)]
pub struct LedgerChanges(pub Map<Address, SetUpdateOrDelete<LedgerEntry, LedgerEntryUpdate>>);

impl Applicable<LedgerChanges> for LedgerChanges {
    /// extends the current `LedgerChanges` with another one
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
    /// Get an item from the `LedgerChanges`
    pub fn get(
        &self,
        addr: &Address,
    ) -> Option<&SetUpdateOrDelete<LedgerEntry, LedgerEntryUpdate>> {
        self.0.get(addr)
    }

    /// Tries to return the parallel balance of an entry
    /// or gets it from a function if the entry's status is unknown.
    ///
    /// This function is used as an optimization:
    /// if the value can be deduced unambiguously from the `LedgerChanges`,
    /// no need to dig further (for example in the `FinalLedger`).
    ///
    /// # Arguments
    /// * `addr`: address for which to get the value
    /// * `f`: fallback function with no arguments and returning `Option<Amount>`
    ///
    /// # Returns
    /// * Some(v) if a value is present, where v is a copy of the value
    /// * None if the value is absent
    /// * f() if the value is unknown
    pub fn get_parallel_balance_or_else<F: FnOnce() -> Option<Amount>>(
        &self,
        addr: &Address,
        f: F,
    ) -> Option<Amount> {
        // Get the changes for the provided address
        match self.0.get(addr) {
            // This entry is being replaced by a new one: get the balance from the new entry
            Some(SetUpdateOrDelete::Set(v)) => Some(v.parallel_balance),

            // This entry is being updated
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                parallel_balance, ..
            })) => match parallel_balance {
                // The update sets a new balance: return it
                SetOrKeep::Set(v) => Some(*v),
                // The update keeps the old balance.
                // We therefore have no info on the absolute value of the balance.
                // We call the fallback function and return its output.
                SetOrKeep::Keep => f(),
            },

            // This entry is being deleted: return None.
            Some(SetUpdateOrDelete::Delete) => None,

            // This entry is not being changed.
            // We therefore have no info on the absolute value of the balance.
            // We call the fallback function and return its output.
            None => f(),
        }
    }

    /// Tries to return the executable bytecode of an entry
    /// or gets it from a function if the entry's status is unknown.
    ///
    /// This function is used as an optimization:
    /// if the value can be deduced unambiguously from the `LedgerChanges`,
    /// no need to dig further (for example in the `FinalLedger`).
    ///
    /// # Arguments
    /// * `addr`: address for which to get the value
    /// * `f`: fallback function with no arguments and returning `Option<Vec<u8>>`
    ///
    /// # Returns
    /// * Some(v) if a value is present, where v is a copy of the value
    /// * None if the value is absent
    /// * f() if the value is unknown
    pub fn get_bytecode_or_else<F: FnOnce() -> Option<Vec<u8>>>(
        &self,
        addr: &Address,
        f: F,
    ) -> Option<Vec<u8>> {
        // Get the changes to the provided address
        match self.0.get(addr) {
            // This entry is being replaced by a new one: get the bytecode from the new entry
            Some(SetUpdateOrDelete::Set(v)) => Some(v.bytecode.clone()),

            // This entry is being updated
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { bytecode, .. })) => match bytecode {
                // The update sets a new bytecode: return it
                SetOrKeep::Set(v) => Some(v.clone()),

                // The update keeps the old bytecode.
                // We therefore have no info on the absolute value of the bytecode.
                // We call the fallback function and return its output.
                SetOrKeep::Keep => f(),
            },

            // This entry is being deleted: return None.
            Some(SetUpdateOrDelete::Delete) => None,

            // This entry is not being changed.
            // We therefore have no info on the absolute contents of the bytecode.
            // We call the fallback function and return its output.
            None => f(),
        }
    }

    /// Tries to return whether an entry exists
    /// or gets the information from a function if the entry's status is unknown.
    ///
    /// This function is used as an optimization:
    /// if the result can be deduced unambiguously from the `LedgerChanges`,
    /// no need to dig further (for example in the `FinalLedger`).
    ///
    /// # Arguments
    /// * `addr`: address to search for
    /// * `f`: fallback function with no arguments and returning a boolean
    ///
    /// # Returns
    /// * true if the entry exists
    /// * false if the value is absent
    /// * f() if the value's existence is unknown
    pub fn entry_exists_or_else<F: FnOnce() -> bool>(&self, addr: &Address, f: F) -> bool {
        // Get the changes for the provided address
        match self.0.get(addr) {
            // The entry is being replaced by a new one: it exists
            Some(SetUpdateOrDelete::Set(_)) => true,

            // The entry is being updated:
            // assume it exists because it will be created on update if it doesn't
            Some(SetUpdateOrDelete::Update(_)) => true,

            // The entry is being deleted: it doesn't exist anymore
            Some(SetUpdateOrDelete::Delete) => false,

            // This entry is not being changed.
            // We therefore have no info on its existence.
            // We call the fallback function and return its output.
            None => f(),
        }
    }

    /// Set the parallel balance of an address.
    /// If the address doesn't exist, its ledger entry is created.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `balance`: parallel balance to set for the provided address
    pub fn set_parallel_balance(&mut self, addr: Address, balance: Amount) {
        // Get the changes for the entry associated to the provided address
        match self.0.entry(addr) {
            // That entry is being changed
            hash_map::Entry::Occupied(mut occ) => {
                match occ.get_mut() {
                    // The entry is being replaced by a new one
                    SetUpdateOrDelete::Set(v) => {
                        // update the parallel_balance of the replacement entry
                        v.parallel_balance = balance;
                    }

                    // The entry is being updated
                    SetUpdateOrDelete::Update(u) => {
                        // Make sure the update sets the parallel balance of the entry to its new value
                        u.parallel_balance = SetOrKeep::Set(balance);
                    }

                    // The entry is being deleted
                    d @ SetUpdateOrDelete::Delete => {
                        // Replace that deletion with a replacement by a new default entry
                        // for which the parallel balance was properly set
                        *d = SetUpdateOrDelete::Set(LedgerEntry {
                            parallel_balance: balance,
                            ..Default::default()
                        });
                    }
                }
            }

            // This entry is not being changed
            hash_map::Entry::Vacant(vac) => {
                // Induce an Update to the entry that sets the balance to its new value
                vac.insert(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    parallel_balance: SetOrKeep::Set(balance),
                    ..Default::default()
                }));
            }
        }
    }

    /// Set the executable bytecode of an address.
    /// If the address doesn't exist, its ledger entry is created.
    ///
    /// # Parameters
    /// * `addr`: target address
    /// * `bytecode`: executable bytecode to assign to that address
    pub fn set_bytecode(&mut self, addr: Address, bytecode: Vec<u8>) {
        // Get the current changes being applied to the entry associated to that address
        match self.0.entry(addr) {
            // There are changes currently being applied to the entry
            hash_map::Entry::Occupied(mut occ) => {
                match occ.get_mut() {
                    // The entry is being replaced by a new one
                    SetUpdateOrDelete::Set(v) => {
                        // update the bytecode of the replacement entry
                        v.bytecode = bytecode;
                    }

                    // The entry is being updated
                    SetUpdateOrDelete::Update(u) => {
                        // Ensure that the update includes setting the bytecode to its new value
                        u.bytecode = SetOrKeep::Set(bytecode);
                    }

                    // The entry is being deleted
                    d @ SetUpdateOrDelete::Delete => {
                        // Replace that deletion with a replacement by a new default entry
                        // for which the bytecode was properly set
                        *d = SetUpdateOrDelete::Set(LedgerEntry {
                            bytecode,
                            ..Default::default()
                        });
                    }
                }
            }

            // This entry is not being changed
            hash_map::Entry::Vacant(vac) => {
                // Induce an Update to the entry that sets the bytecode to its new value
                vac.insert(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    bytecode: SetOrKeep::Set(bytecode),
                    ..Default::default()
                }));
            }
        }
    }

    /// Tries to return a datastore entry for a given address,
    /// or gets it from a function if the value's status is unknown.
    ///
    /// This function is used as an optimization:
    /// if the result can be deduced unambiguously from the `LedgerChanges`,
    /// no need to dig further (for example in the `FinalLedger`).
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    /// * `f`: fallback function with no arguments and returning `Option<Vec<u8>>`
    ///
    /// # Returns
    /// * Some(v) if the value was found, where v is a copy of the value
    /// * None if the value is absent
    /// * f() if the value is unknown
    pub fn get_data_entry_or_else<F: FnOnce() -> Option<Vec<u8>>>(
        &self,
        addr: &Address,
        key: &Hash,
        f: F,
    ) -> Option<Vec<u8>> {
        // Get the current changes being applied to the ledger entry associated to that address
        match self.0.get(addr) {
            // This ledger entry is being replaced by a new one:
            // get the datastore entry from the new ledger entry
            Some(SetUpdateOrDelete::Set(v)) => v.datastore.get(key).cloned(),

            // This ledger entry is being updated
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { datastore, .. })) => {
                // Get the update being applied to that datastore entry
                match datastore.get(key) {
                    // A new datastore value is being set: return a clone of it
                    Some(SetOrDelete::Set(v)) => Some(v.clone()),

                    // This datastore entry is being deleted: return None
                    Some(SetOrDelete::Delete) => None,

                    // There are no changes to this particular datastore entry.
                    // We therefore have no info on the absolute contents of the datastore entry.
                    // We call the fallback function and return its output.
                    None => f(),
                }
            }

            // This ledger entry is being deleted: return None
            Some(SetUpdateOrDelete::Delete) => None,

            // This ledger entry is not being changed.
            // We therefore have no info on the absolute contents of its datastore entry.
            // We call the fallback function and return its output.
            None => f(),
        }
    }

    /// Tries to return whether a datastore entry exists for a given address,
    /// or gets it from a function if the datastore entry's status is unknown.
    ///
    /// This function is used as an optimization:
    /// if the result can be deduced unambiguously from the `LedgerChanges`,
    /// no need to dig further (for example in the `FinalLedger`).
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    /// * `f`: fallback function with no arguments and returning a boolean
    ///
    /// # Returns
    /// * true if the ledger entry exists and the key is present in its datastore
    /// * false if the ledger entry is absent, or if the key is not in its datastore
    /// * f() if the existence of the ledger entry or datastore entry is unknown
    pub fn has_data_entry_or_else<F: FnOnce() -> bool>(
        &self,
        addr: &Address,
        key: &Hash,
        f: F,
    ) -> bool {
        // Get the current changes being applied to the ledger entry associated to that address
        match self.0.get(addr) {
            // This ledger entry is being replaced by a new one:
            // check if the replacement ledger entry has the key in its datastore
            Some(SetUpdateOrDelete::Set(v)) => v.datastore.contains_key(key),

            // This ledger entry is being updated
            Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { datastore, .. })) => {
                // Get the update being applied to that datastore entry
                match datastore.get(key) {
                    // A new datastore value is being set: the datastore entry exists
                    Some(SetOrDelete::Set(_)) => true,

                    // The datastore entry is being deletes: it doesn't exist anymore
                    Some(SetOrDelete::Delete) => false,

                    // There are no changes to this particular datastore entry.
                    // We therefore have no info on its existence.
                    // We call the fallback function and return its output.
                    None => f(),
                }
            }

            // This ledger entry is being deleted: it has no datastore anymore
            Some(SetUpdateOrDelete::Delete) => false,

            // This ledger entry is not being changed.
            // We therefore have no info on its datastore.
            // We call the fallback function and return its output.
            None => f(),
        }
    }

    /// Set a datastore entry for a given address.
    /// If the address doesn't exist, its ledger entry is created.
    /// If the datastore entry exists, its value is replaced, otherwise it is created.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    /// * `data`: datastore value to set
    pub fn set_data_entry(&mut self, addr: Address, key: Hash, data: Vec<u8>) {
        // Get the changes being applied to the ledger entry associated to that address
        match self.0.entry(addr) {
            // There are changes currently being applied to the ledger entry
            hash_map::Entry::Occupied(mut occ) => {
                match occ.get_mut() {
                    // The ledger entry is being replaced by a new one
                    SetUpdateOrDelete::Set(v) => {
                        // Insert the value in the datastore of the replacement entry
                        // Any existing value is overwritten
                        v.datastore.insert(key, data);
                    }

                    // The ledger entry is being updated
                    SetUpdateOrDelete::Update(u) => {
                        // Ensure that the update includes setting the datastore entry
                        u.datastore.insert(key, SetOrDelete::Set(data));
                    }

                    // The ledger entry is being deleted
                    d @ SetUpdateOrDelete::Delete => {
                        // Replace that ledger entry deletion with a replacement by a new default ledger entry
                        // for which the datastore contains the (key, value) to insert.
                        *d = SetUpdateOrDelete::Set(LedgerEntry {
                            datastore: vec![(key, data)].into_iter().collect(),
                            ..Default::default()
                        });
                    }
                }
            }

            // This ledger entry is not being changed
            hash_map::Entry::Vacant(vac) => {
                // Induce an Update to the ledger entry that sets the datastore entry to the desired value
                vac.insert(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    datastore: vec![(key, SetOrDelete::Set(data))].into_iter().collect(),
                    ..Default::default()
                }));
            }
        }
    }
}
