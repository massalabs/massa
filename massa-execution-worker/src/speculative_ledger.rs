// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative ledger represents, in a compressed way,
//! the state of the ledger at an arbitrary execution slot.
//! It never actually writes to the consensus state
//! but keeps track of the changes that were applied to it since its creation.

use massa_execution_exports::ExecutionError;
use massa_hash::hash::Hash;
use massa_ledger::{Applicable, FinalLedger, LedgerChanges};
use massa_models::{Address, Amount};
use parking_lot::RwLock;
use std::sync::Arc;

/// The SpeculativeLedger contains an thread-safe shared reference to the final ledger (read-only),
/// a list of existing changes that happened o the ledger since its finality,
/// as well as an extra list of "added" changes.
/// The SpeculativeLedger makes it possible to transparently manipulate a virtual ledger
/// that takes into account all those ledger changes and allows adding more
/// while keeping track of all the newly added changes, and never writing in the final ledger.
pub struct SpeculativeLedger {
    /// Thread-safe shared access to the final ledger. For reading only.
    final_ledger: Arc<RwLock<FinalLedger>>,

    /// Accumulation of changes that previously happened to the ledger since finality.
    /// This value is not modified by changes applied to the SpeculativeLedger.
    ///
    /// TODO maybe have the history directly here,
    /// so that we can avoid accumulating all the changes at every slot
    /// but only lazily query addresses backwards in history (to avoid useless computations) with caching
    previous_changes: LedgerChanges,

    /// list of ledger changes that were applied to this SpeculativeLedger since its creation
    added_changes: LedgerChanges,
}

impl SpeculativeLedger {
    /// creates a new SpeculativeLedger
    ///
    /// # Arguments
    /// * final_ledger: thread-safe shared access to the final ledger (for reading only)
    /// * previous_changes: accumulation of changes that previously happened to the ledger since finality
    pub fn new(final_ledger: Arc<RwLock<FinalLedger>>, previous_changes: LedgerChanges) -> Self {
        SpeculativeLedger {
            final_ledger,
            previous_changes,
            added_changes: Default::default(),
        }
    }

    /// Returns the changes caused to the SpeculativeLedger since its creation,
    /// and resets their local value to nothing.
    pub fn take(&mut self) -> LedgerChanges {
        std::mem::take(&mut self.added_changes)
    }

    /// Takes a snapshot (clone) of the changes caused to the SpeculativeLedger since its creation
    pub fn get_snapshot(&self) -> LedgerChanges {
        self.added_changes.clone()
    }

    /// Resets the SpeculativeLedger to a snapshot (see get_snapshot method)
    pub fn reset_to_snapshot(&mut self, snapshot: LedgerChanges) {
        self.added_changes = snapshot;
    }

    /// Gets the effective parallel balance of an address
    ///
    /// # Arguments:
    /// addr: the address to query
    ///
    /// # Returns
    /// Some(Amount) if the address was found, otherwise None
    pub fn get_parallel_balance(&self, addr: &Address) -> Option<Amount> {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.get_parallel_balance_or_else(addr, || {
            self.previous_changes
                .get_parallel_balance_or_else(addr, || {
                    self.final_ledger.read().get_parallel_balance(addr)
                })
        })
    }

    /// Gets the effective bytecode of an address
    ///
    /// # Arguments:
    /// addr: the address to query
    ///
    /// # Returns
    /// Some(Vec<u8>) if the address was found, otherwise None
    pub fn get_bytecode(&self, addr: &Address) -> Option<Vec<u8>> {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.get_bytecode_or_else(addr, || {
            self.previous_changes
                .get_bytecode_or_else(addr, || self.final_ledger.read().get_bytecode(addr))
        })
    }

    /// Transfers parallel coins from one address to another.
    /// No changes are retained in case of failure.
    /// The spending address, if defined, must exist.
    ///
    /// # parameters
    /// * from_addr: optional spending address (use None for pure coin creation)
    /// * to_addr: optional crediting address (use None for pure coin destruction)
    /// * amount: amount of coins to transfer
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
                    ExecutionError::RuntimeError("unsufficient from_addr balance".into())
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
    /// addr: the address to query
    ///
    /// # Returns
    /// true if the address was found, otherwise false
    pub fn entry_exists(&self, addr: &Address) -> bool {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.entry_exists_or_else(addr, || {
            self.previous_changes
                .entry_exists_or_else(addr, || self.final_ledger.read().entry_exists(addr))
        })
    }

    /// Creates a new smart contract address with initial bytecode.
    ///
    /// # Arguments
    /// * addr: address to create
    /// * bytecode: bytecode to set in the new ledger entry
    pub fn create_new_sc_address(
        &mut self,
        addr: Address,
        bytecode: Vec<u8>,
    ) -> Result<(), ExecutionError> {
        // set bytecode (create if non-existant)
        self.added_changes.set_bytecode(addr, bytecode);
        Ok(())
    }

    /// Sets the bytecode associated to an address in the ledger.
    /// Fails if the address doesn't exist.
    ///
    /// # Arguments
    /// * addr: target address
    /// * bytecode: bytecode to set for that address
    #[allow(dead_code)] // TODO remove when it is used
    pub fn set_bytecode(&mut self, addr: Address, bytecode: Vec<u8>) -> Result<(), ExecutionError> {
        // check for address existence
        if !self.entry_exists(&addr) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set bytecode for address {}: entry does not exist",
                addr
            )));
        }

        // set the bytecode of that address
        self.added_changes.set_bytecode(addr, bytecode);

        Ok(())
    }

    /// Gets a copy of a datastore value for a given address and datastore key
    ///
    /// # Arguments
    /// * addr: address to query
    /// * key: key to query in the address' datastore
    ///
    /// # Returns
    /// Some(Vec<u8>) if the value was found, None if the address does not exist or if the key is not in its datastore.
    pub fn get_data_entry(&self, addr: &Address, key: &Hash) -> Option<Vec<u8>> {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.get_data_entry_or_else(addr, key, || {
            self.previous_changes.get_data_entry_or_else(addr, key, || {
                self.final_ledger.read().get_data_entry(addr, key)
            })
        })
    }

    /// Checks if a data entry exists for a given address
    ///
    /// # Arguments
    /// * addr: address to query
    /// * key: datastore key to look for
    ///
    /// # Returns
    /// true if the key exists in the address' datastore, false otherwise
    pub fn has_data_entry(&self, addr: &Address, key: &Hash) -> bool {
        // try to read from added_changes, then previous_changes, then final_ledger
        self.added_changes.has_data_entry_or_else(addr, key, || {
            self.previous_changes.has_data_entry_or_else(addr, key, || {
                self.final_ledger.read().has_data_entry(addr, key)
            })
        })
    }

    /// Sets a dataset entry for a given address in the ledger.
    /// Fails if the address doesn't exist.
    /// If the datastore entry does not exist, it is created.
    ///
    /// # Arguments
    /// * addr: target address
    /// * key: datastore key
    /// * data: value to associate to the datasotre key
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
}
