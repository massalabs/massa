// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative ledger represents, in a compressed way,
//! the state of the ledger at an arbitrary execution slot.
//! It never actually writes to the consensus state
//! but keeps track of the changes that were applied to it since its creation.

use massa_execution_exports::{ExecutionError, ExecutionOutput};
use massa_final_state::FinalState;
use massa_hash::Hash;
use massa_ledger_exports::{
    Applicable, LedgerChanges, LedgerEntry, LedgerEntryUpdate, SetOrDelete, SetOrKeep,
    SetUpdateOrDelete,
};
use massa_models::{Address, Amount};
use parking_lot::RwLock;
use std::{collections::VecDeque, sync::Arc};

pub enum HistorySearchResult<T> {
    Found(T),
    NotFound,
    Deleted,
}

/// The `SpeculativeLedger` contains an thread-safe shared reference to the final ledger (read-only),
/// a list of existing changes that happened o the ledger since its finality,
/// as well as an extra list of "added" changes.
/// The `SpeculativeLedger` makes it possible to transparently manipulate a virtual ledger
/// that takes into account all those ledger changes and allows adding more
/// while keeping track of all the newly added changes, and never writing in the final ledger.
pub struct SpeculativeLedger {
    /// Thread-safe shared access to the final state. For reading only.
    final_state: Arc<RwLock<FinalState>>,

    /// History of the outputs of recently executed slots.
    /// Slots should be consecutive, newest at the back.
    active_history: Arc<RwLock<VecDeque<ExecutionOutput>>>,

    /// list of ledger changes that were applied to this `SpeculativeLedger` since its creation
    added_changes: LedgerChanges,
}

impl SpeculativeLedger {
    /// creates a new `SpeculativeLedger`
    ///
    /// # Arguments
    /// * `final_state`: thread-safe shared access to the final state (for reading only)
    /// * `previous_changes`: accumulation of changes that previously happened to the ledger since finality
    pub fn new(
        final_state: Arc<RwLock<FinalState>>,
        active_history: Arc<RwLock<VecDeque<ExecutionOutput>>>,
    ) -> Self {
        SpeculativeLedger {
            final_state,
            added_changes: Default::default(),
            active_history,
        }
    }

    /// Lazily query (from end to beginning) the active balance of an address at a given slot.
    ///
    /// Returns None if the address balance could not be determined from the active history.
    pub fn fetch_active_history_balance(&self, addr: &Address) -> HistorySearchResult<Amount> {
        let hist = self.active_history.read();
        let iter = hist.iter().rev();

        for output in iter {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(v)) => {
                    return HistorySearchResult::Found(v.parallel_balance)
                }
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    parallel_balance: SetOrKeep::Set(v),
                    ..
                })) => return HistorySearchResult::Found(*v),
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Deleted,
                _ => (),
            }
        }
        HistorySearchResult::NotFound
    }

    /// Lazily query (from end to beginning) the active balance of an address at a given slot.
    ///
    /// Returns None if the address balance could not be determined from the active history.
    pub fn fetch_active_history_bytecode(&self, addr: &Address) -> HistorySearchResult<Vec<u8>> {
        let hist = self.active_history.read();
        let iter = hist.iter().rev();

        for output in iter {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(v)) => return HistorySearchResult::Found(v.bytecode),
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate {
                    bytecode: SetOrKeep::Set(v),
                    ..
                })) => return HistorySearchResult::Found(*v),
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Deleted,
                _ => (),
            }
        }
        HistorySearchResult::NotFound
    }

    /// Lazily query (from end to beginning) the active datastore entry of an address at a given slot.
    ///
    /// Returns None if the datastore entry could not be determined from the active history.
    pub fn fetch_active_history_data_entry(
        &self,
        addr: &Address,
        key: &Hash,
    ) -> HistorySearchResult<Vec<u8>> {
        let hist = self.active_history.read();
        let iter = hist.iter().rev();

        for output in iter {
            match output.state_changes.ledger_changes.0.get(addr) {
                Some(SetUpdateOrDelete::Set(LedgerEntry { datastore, .. })) => {
                    match datastore.get(key) {
                        Some(value) => return HistorySearchResult::Found(value.to_vec()),
                        None => (),
                    }
                }
                Some(SetUpdateOrDelete::Update(LedgerEntryUpdate { datastore, .. })) => {
                    match datastore.get(key) {
                        Some(SetOrDelete::Set(value)) => {
                            return HistorySearchResult::Found(value.to_vec())
                        }
                        Some(SetOrDelete::Delete) => return HistorySearchResult::Deleted,
                        None => (),
                    }
                }
                Some(SetUpdateOrDelete::Delete) => return HistorySearchResult::Deleted,
                None => (),
            }
        }
        HistorySearchResult::NotFound
    }

    /// Returns the changes caused to the `SpeculativeLedger` since its creation,
    /// and resets their local value to nothing.
    pub fn take(&mut self) -> LedgerChanges {
        std::mem::take(&mut self.added_changes)
    }

    /// Takes a snapshot (clone) of the changes caused to the `SpeculativeLedger` since its creation
    pub fn get_snapshot(&self) -> LedgerChanges {
        self.added_changes.clone()
    }

    /// Resets the `SpeculativeLedger` to a snapshot (see `get_snapshot` method)
    pub fn reset_to_snapshot(&mut self, snapshot: LedgerChanges) {
        self.added_changes = snapshot;
    }

    /// Gets the effective parallel balance of an address
    ///
    /// # Arguments:
    /// `addr`: the address to query
    ///
    /// # Returns
    /// Some(Amount) if the address was found, otherwise None
    pub fn get_parallel_balance(&self, addr: &Address) -> Option<Amount> {
        // try to read from history, then final_state
        let search_result = self.fetch_active_history_balance(addr);
        match search_result {
            HistorySearchResult::Found(active_balance) => Some(active_balance),
            HistorySearchResult::NotFound => {
                self.final_state.read().ledger.get_parallel_balance(addr)
            }
            // NOTE: not sure about the deleted behaviour
            HistorySearchResult::Deleted => None,
        }
    }

    /// Gets the effective bytecode of an address
    ///
    /// # Arguments:
    /// `addr`: the address to query
    ///
    /// # Returns
    /// `Some(Vec<u8>)` if the address was found, otherwise None
    pub fn get_bytecode(&self, addr: &Address) -> Option<Vec<u8>> {
        // try to read from history, then final_state
        let search_result = self.fetch_active_history_bytecode(addr);
        match search_result {
            HistorySearchResult::Found(bytecode) => Some(bytecode),
            HistorySearchResult::NotFound => self.final_state.read().ledger.get_bytecode(addr),
            // NOTE: not sure about the deleted behaviour
            HistorySearchResult::Deleted => None,
        }
    }

    /// Transfers parallel coins from one address to another.
    /// No changes are retained in case of failure.
    /// The spending address, if defined, must exist.
    ///
    /// # Parameters:
    /// * `from_addr`: optional spending address (use None for pure coin creation)
    /// * `to_addr`: optional crediting address (use None for pure coin destruction)
    /// * `amount`: amount of coins to transfer
    pub fn transfer_parallel_coins(
        &mut self,
        from_addr: Option<Address>,
        to_addr: Option<Address>,
        amount: Amount,
    ) -> Result<(), ExecutionError> {
        // init empty ledger changes
        let mut changes = LedgerChanges::default();

        // simulate spending coins from sender address (if any)
        if let Some(from_addr) = from_addr {
            let new_balance = self
                .get_parallel_balance(&from_addr)
                .ok_or_else(|| ExecutionError::RuntimeError("source address not found".into()))?
                .checked_sub(amount)
                .ok_or_else(|| {
                    ExecutionError::RuntimeError("insufficient from_addr balance".into())
                })?;
            changes.set_parallel_balance(from_addr, new_balance);
        }

        // simulate crediting coins to destination address (if any)
        // note that to_addr can be the same as from_addr
        if let Some(to_addr) = to_addr {
            let new_balance = changes
                .get_parallel_balance_or_else(&to_addr, || self.get_parallel_balance(&to_addr))
                .unwrap_or_default()
                .checked_add(amount)
                .ok_or_else(|| {
                    ExecutionError::RuntimeError("overflow in to_addr balance".into())
                })?;
            changes.set_parallel_balance(to_addr, new_balance);
        }

        // apply the simulated changes to the speculative ledger
        self.added_changes.apply(changes);

        Ok(())
    }

    /// Checks if an address exists in the speculative ledger
    ///
    /// # Arguments:
    /// `addr`: the address to query
    ///
    /// # Returns
    /// true if the address was found, otherwise false
    pub fn entry_exists(&self, addr: &Address) -> bool {
        // try to read from added_changes, then previous_changes, then ledger in final_state
        self.added_changes.entry_exists_or_else(addr, || {
            self.previous_changes
                .entry_exists_or_else(addr, || self.final_state.read().ledger.entry_exists(addr))
        })
    }

    /// Creates a new smart contract address with initial bytecode.
    ///
    /// # Arguments
    /// * `addr`: address to create
    /// * `bytecode`: bytecode to set in the new ledger entry
    pub fn create_new_sc_address(
        &mut self,
        addr: Address,
        bytecode: Vec<u8>,
    ) -> Result<(), ExecutionError> {
        // set bytecode (create if do not exist)
        self.added_changes.set_bytecode(addr, bytecode);
        Ok(())
    }

    /// Sets the bytecode associated to an address in the ledger.
    /// Fails if the address doesn't exist.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `bytecode`: bytecode to set for that address
    pub fn set_bytecode(
        &mut self,
        addr: &Address,
        bytecode: Vec<u8>,
    ) -> Result<(), ExecutionError> {
        // check for address existence
        if !self.entry_exists(addr) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set bytecode for address {}: entry does not exist",
                addr
            )));
        }

        // set the bytecode of that address
        self.added_changes.set_bytecode(*addr, bytecode);

        Ok(())
    }

    /// Gets a copy of a datastore value for a given address and datastore key
    ///
    /// # Arguments
    /// * `addr`: address to query
    /// * `key`: key to query in the address' datastore
    ///
    /// # Returns
    /// `Some(Vec<u8>)` if the value was found, `None` if the address does not exist or if the key is not in its datastore.
    pub fn get_data_entry(&self, addr: &Address, key: &Hash) -> Option<Vec<u8>> {
        // try to read from history, then ledger in final_state
        let search_result = self.fetch_active_history_data_entry(addr, key);
        match search_result {
            HistorySearchResult::Found(entry) => Some(entry),
            HistorySearchResult::NotFound => {
                self.final_state.read().ledger.get_data_entry(addr, key)
            }
            // NOTE: not sure about the deleted behaviour
            HistorySearchResult::Deleted => None,
        }
    }

    /// Checks if a data entry exists for a given address
    ///
    /// # Arguments
    /// * `addr`: address to query
    /// * `key`: datastore key to look for
    ///
    /// # Returns
    /// true if the key exists in the address datastore, false otherwise
    pub fn has_data_entry(&self, addr: &Address, key: &Hash) -> bool {
        // try to read from added_changes, then previous_changes, then ledger in final_state
        self.added_changes.has_data_entry_or_else(addr, key, || {
            self.previous_changes.has_data_entry_or_else(addr, key, || {
                self.final_state.read().ledger.has_data_entry(addr, key)
            })
        })
    }

    /// Sets a data set entry for a given address in the ledger.
    /// Fails if the address doesn't exist.
    /// If the datastore entry does not exist, it is created.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    /// * `data`: value to associate to the datastore key
    pub fn set_data_entry(
        &mut self,
        addr: &Address,
        key: Hash,
        data: Vec<u8>,
    ) -> Result<(), ExecutionError> {
        // check for address existence
        if !self.entry_exists(addr) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set data for address {}: entry does not exist",
                addr
            )));
        }

        // set data
        self.added_changes.set_data_entry(*addr, key, data);

        Ok(())
    }

    /// Deletes a datastore entry for a given address.
    /// Fails if the entry or address does not exist.
    ///
    /// # Arguments
    /// * `addr`: address
    /// * `key`: key of the entry to delete in the address' datastore
    pub fn delete_data_entry(&mut self, addr: &Address, key: &Hash) -> Result<(), ExecutionError> {
        // check if the entry exists
        if !self.has_data_entry(addr, key) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not delete data entry {} for address {}: entry does not exist",
                key, addr
            )));
        }

        // delete entry
        self.added_changes.delete_data_entry(*addr, *key);

        Ok(())
    }
}
