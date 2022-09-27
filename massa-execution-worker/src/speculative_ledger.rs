// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative ledger represents, in a compressed way,
//! the state of the ledger at an arbitrary execution slot.
//! It never actually writes to the consensus state
//! but keeps track of the changes that were applied to it since its creation.

use crate::active_history::{ActiveHistory, HistorySearchResult};
use massa_execution_exports::ExecutionError;
use massa_execution_exports::StorageCostsConstants;
use massa_final_state::FinalState;
use massa_ledger_exports::{Applicable, LedgerChanges};
use massa_models::{address::Address, amount::Amount};
use parking_lot::RwLock;
use std::cmp::Ordering;
use std::sync::Arc;
use tracing::log::debug;

/// The `SpeculativeLedger` contains an thread-safe shared reference to the final ledger (read-only),
/// a list of existing changes that happened o the ledger since its finality,
/// as well as an extra list of "added" changes.
/// The `SpeculativeLedger` makes it possible to transparently manipulate a virtual ledger
/// that takes into account all those ledger changes and allows adding more
/// while keeping track of all the newly added changes, and never writing in the final ledger.
pub(crate) struct SpeculativeLedger {
    /// Thread-safe shared access to the final state. For reading only.
    final_state: Arc<RwLock<FinalState>>,

    /// History of the outputs of recently executed slots.
    /// Slots should be consecutive, newest at the back.
    active_history: Arc<RwLock<ActiveHistory>>,

    /// list of ledger changes that were applied to this `SpeculativeLedger` since its creation
    added_changes: LedgerChanges,

    /// max datastore key length
    max_datastore_key_length: u8,

    /// Max datastore value size
    max_datastore_value_size: u64,

    /// Max bytecode size
    max_bytecode_size: u64,

    /// storage cost constants
    storage_costs_constants: StorageCostsConstants,
}

impl SpeculativeLedger {
    /// creates a new `SpeculativeLedger`
    ///
    /// # Arguments
    /// * `final_state`: thread-safe shared access to the final state (for reading only)
    /// * `active_history`: thread-safe shared access the speculative execution history
    pub fn new(
        final_state: Arc<RwLock<FinalState>>,
        active_history: Arc<RwLock<ActiveHistory>>,
        max_datastore_key_length: u8,
        max_bytecode_size: u64,
        max_datastore_value_size: u64,
        storage_costs_constants: StorageCostsConstants,
    ) -> Self {
        SpeculativeLedger {
            final_state,
            added_changes: Default::default(),
            active_history,
            max_datastore_key_length,
            max_datastore_value_size,
            max_bytecode_size,
            storage_costs_constants,
        }
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

    /// Gets the effective balance of an address
    ///
    /// # Arguments:
    /// `addr`: the address to query
    ///
    /// # Returns
    /// Some(Amount) if the address was found, otherwise None
    pub fn get_balance(&self, addr: &Address) -> Option<Amount> {
        // try to read from added changes > history > final_state
        self.added_changes.get_balance_or_else(addr, || {
            match self.active_history.read().fetch_balance(addr) {
                HistorySearchResult::Present(par_balance) => Some(par_balance),
                HistorySearchResult::NoInfo => self.final_state.read().ledger.get_balance(addr),
                HistorySearchResult::Absent => None,
            }
        })
    }

    /// Gets the effective bytecode of an address
    ///
    /// # Arguments:
    /// `addr`: the address to query
    ///
    /// # Returns
    /// `Some(Vec<u8>)` if the address was found, otherwise None
    pub fn get_bytecode(&self, addr: &Address) -> Option<Vec<u8>> {
        // try to read from added changes > history > final_state
        self.added_changes.get_bytecode_or_else(addr, || {
            match self.active_history.read().fetch_bytecode(addr) {
                HistorySearchResult::Present(bytecode) => Some(bytecode),
                HistorySearchResult::NoInfo => self.final_state.read().ledger.get_bytecode(addr),
                HistorySearchResult::Absent => None,
            }
        })
    }

    /// Transfers coins from one address to another.
    /// No changes are retained in case of failure.
    /// The spending address, if defined, must exist.
    ///
    /// # Parameters:
    /// * `from_addr`: optional spending address (use None for pure coin creation)
    /// * `to_addr`: optional crediting address (use None for pure coin destruction)
    /// * `amount`: amount of coins to transfer
    pub fn transfer_coins(
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
                .get_balance(&from_addr)
                .ok_or_else(|| ExecutionError::RuntimeError("source addr not found".to_string()))?
                .checked_sub(amount)
                .ok_or_else(|| {
                    ExecutionError::RuntimeError("insufficient from_addr balance".into())
                })?;
            changes.set_balance(from_addr, new_balance);
        }

        // simulate crediting coins to destination address (if any)
        // note that to_addr can be the same as from_addr
        if let Some(to_addr) = to_addr {
            let old_balance = changes.get_balance_or_else(&to_addr, || self.get_balance(&to_addr));
            match (old_balance, from_addr) {
                // if `to_addr` exists we increase the balance
                (Some(old_balance), _) => {
                    let new_balance = old_balance.checked_add(amount).ok_or_else(|| {
                        ExecutionError::RuntimeError("overflow in to_addr balance".into())
                    })?;
                    changes.set_balance(to_addr, new_balance);
                }
                // if `to_addr` doesn't exist but `from_addr` is defined. `from_addr` will create the address using the coins sent.
                (None, Some(_)) => {
                    //TODO: Remove when stabilized
                    debug!("Creating address {} from coins in transactions", to_addr);
                    if amount >= self.storage_costs_constants.ledger_entry_base_cost {
                        changes.set_balance(
                            to_addr,
                            amount
                                .checked_sub(self.storage_costs_constants.ledger_entry_base_cost)
                                .ok_or_else(|| {
                                    ExecutionError::RuntimeError(
                                        "overflow in subtract ledger cost for addr".to_string(),
                                    )
                                })?,
                        );
                    } else {
                        return Err(ExecutionError::RuntimeError(
                            "insufficient amount to create receiver address".to_string(),
                        ));
                    }
                }
                // if `from_addr` is none and `to_addr` doesn't exist try to create it from coins sent
                (None, None) => {
                    //TODO: Remove when stabilized
                    debug!("Creating address {} from coins generated", to_addr);
                    // We have enough to create the address and transfer the rest.
                    if amount >= self.storage_costs_constants.ledger_entry_base_cost {
                        changes.set_balance(
                            to_addr,
                            amount
                                .checked_sub(self.storage_costs_constants.ledger_entry_base_cost)
                                .ok_or_else(|| {
                                    ExecutionError::RuntimeError(
                                        "overflow in subtract ledger cost for addr".to_string(),
                                    )
                                })?,
                        );
                    } else {
                        return Ok(());
                    }
                }
            };
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
        // try to read from added changes > history > final_state
        self.added_changes.entry_exists_or_else(addr, || {
            match self.active_history.read().fetch_balance(addr) {
                HistorySearchResult::Present(_balance) => true,
                HistorySearchResult::NoInfo => self.final_state.read().ledger.entry_exists(addr),
                HistorySearchResult::Absent => false,
            }
        })
    }

    /// Creates a new smart contract address with initial bytecode.
    ///
    /// # Arguments
    /// * `creator_address`: address that asked for this creation. Will pay the storage costs.
    /// * `addr`: address to create
    /// * `bytecode`: bytecode to set in the new ledger entry
    pub fn create_new_sc_address(
        &mut self,
        creator_address: Address,
        addr: Address,
        bytecode: Vec<u8>,
    ) -> Result<(), ExecutionError> {
        // check for address existence
        if !self.entry_exists(&creator_address) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set bytecode for address {}: entry does not exist",
                addr
            )));
        }

        // check that we don't collide with existing address
        if self.entry_exists(&addr) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set bytecode for address {}: address generated to store bytecode already exists. Try again.",
                addr
            )));
        }

        if bytecode.len() > self.max_bytecode_size as usize {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set bytecode for address {}: bytecode size exceeds maximum allowed size",
                addr
            )));
        }

        // calculate the cost of storing the address and bytecode
        let address_storage_cost = self
            .storage_costs_constants
            .ledger_entry_base_cost
            // Bytecode data
            .checked_add(
                self.storage_costs_constants
                    .ledger_cost_per_byte
                    .checked_mul_u64(bytecode.len().try_into().map_err(|_| {
                        ExecutionError::RuntimeError(
                            "overflow calculating size key for bytecode".to_string(),
                        )
                    })?)
                    .ok_or_else(|| {
                        ExecutionError::RuntimeError(
                            "overflow in ledger cost for bytecode".to_string(),
                        )
                    })?,
            )
            .ok_or_else(|| {
                ExecutionError::RuntimeError("overflow in ledger cost for bytecode".to_string())
            })?;

        self.transfer_coins(Some(creator_address), None, address_storage_cost)?;
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

        if bytecode.len() > self.max_bytecode_size as usize {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set bytecode for address {}: bytecode size exceeds maximum allowed size",
                addr
            )));
        }

        if let Some(old_bytecode_size) = self.get_bytecode(addr).map(|b| b.len()) {
            let diff_size_storage: i64 = (bytecode.len() as i64) - (old_bytecode_size as i64);
            let storage_cost_bytecode = self
                .storage_costs_constants
                .ledger_cost_per_byte
                .checked_mul_u64(diff_size_storage.unsigned_abs())
                .ok_or_else(|| {
                    ExecutionError::RuntimeError("try to store too much data".to_string())
                })?;

            match diff_size_storage.cmp(&0) {
                Ordering::Greater => {
                    self.transfer_coins(Some(*addr), None, storage_cost_bytecode)?
                }
                Ordering::Less => self.transfer_coins(None, Some(*addr), storage_cost_bytecode)?,
                Ordering::Equal => {}
            };
        } else {
            let bytecode_storage_cost = self
                .storage_costs_constants
                .ledger_cost_per_byte
                .checked_mul_u64(bytecode.len() as u64)
                .ok_or_else(|| {
                    ExecutionError::RuntimeError(
                        "overflow when calculating storage cost of bytecode".to_string(),
                    )
                })?;
            self.transfer_coins(Some(*addr), None, bytecode_storage_cost)?;
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
    pub fn get_data_entry(&self, addr: &Address, key: &[u8]) -> Option<Vec<u8>> {
        // try to read from added changes > history > final_state
        self.added_changes.get_data_entry_or_else(addr, key, || {
            match self
                .active_history
                .read()
                .fetch_active_history_data_entry(addr, key)
            {
                HistorySearchResult::Present(entry) => Some(entry),
                HistorySearchResult::NoInfo => {
                    self.final_state.read().ledger.get_data_entry(addr, key)
                }
                HistorySearchResult::Absent => None,
            }
        })
    }

    /// Checks if a data entry exists for a given address
    ///
    /// # Arguments
    /// * `addr`: address to query
    /// * `key`: datastore key to look for
    ///
    /// # Returns
    /// true if the key exists in the address datastore, false otherwise
    pub fn has_data_entry(&self, addr: &Address, key: &[u8]) -> bool {
        // try to read from added changes > history > final_state
        self.added_changes.has_data_entry_or_else(addr, key, || {
            match self
                .active_history
                .read()
                .fetch_active_history_data_entry(addr, key)
            {
                HistorySearchResult::Present(_entry) => true,
                HistorySearchResult::NoInfo => {
                    self.final_state.read().ledger.has_data_entry(addr, key)
                }
                HistorySearchResult::Absent => false,
            }
        })
    }

    fn get_storage_cost_datastore_value(&self, value: &Vec<u8>) -> Result<Amount, ExecutionError> {
        self.storage_costs_constants
            .ledger_cost_per_byte
            .checked_mul_u64(value.len().try_into().map_err(|_| {
                ExecutionError::RuntimeError("value in datastore is too big".to_string())
            })?)
            .ok_or_else(|| {
                ExecutionError::RuntimeError(
                    "overflow when calculating storage cost for datastore value".to_string(),
                )
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
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<(), ExecutionError> {
        // check for address existence
        if !self.entry_exists(addr) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set data for address {}: entry does not exist",
                addr
            )));
        }

        // check key correctness
        let key_length = key.len();
        if key_length == 0 || key_length > self.max_datastore_key_length as usize {
            return Err(ExecutionError::RuntimeError(format!(
                "key length is {}, but it must be in [0..={}]",
                key_length, self.max_datastore_key_length
            )));
        }

        if value.len() > self.max_datastore_value_size as usize {
            return Err(ExecutionError::RuntimeError(format!(
                "value length is {}, but it must be in [0..={}]",
                value.len(),
                self.max_datastore_value_size
            )));
        }

        // Debit the cost of the key if it is a new one
        // and the cost of value if new or if it change
        if let Some(old_value) = self.get_data_entry(addr, &key) {
            let diff_size_storage: i64 = (value.len() as i64) - (old_value.len() as i64);
            let storage_cost_value = self
                .storage_costs_constants
                .ledger_cost_per_byte
                .checked_mul_u64(diff_size_storage.unsigned_abs())
                .ok_or_else(|| {
                    ExecutionError::RuntimeError("try to store too much data".to_string())
                })?;
            match diff_size_storage.cmp(&0) {
                Ordering::Greater => self.transfer_coins(Some(*addr), None, storage_cost_value)?,
                Ordering::Less => self.transfer_coins(None, Some(*addr), storage_cost_value)?,
                Ordering::Equal => {}
            };
        } else {
            let value_storage_cost = self.get_storage_cost_datastore_value(&value)?;
            self.transfer_coins(
                Some(*addr),
                None,
                self.storage_costs_constants
                    .ledger_entry_datastore_base_cost
                    .checked_add(value_storage_cost)
                    .ok_or_else(|| {
                        ExecutionError::RuntimeError(
                            "overflow when calculating storage cost for datastore key/value"
                                .to_string(),
                        )
                    })?,
            )?;
        }

        // set data
        self.added_changes.set_data_entry(*addr, key, value);

        Ok(())
    }

    /// Deletes a datastore entry for a given address.
    /// Fails if the entry or address does not exist.
    ///
    /// # Arguments
    /// * `addr`: address
    /// * `key`: key of the entry to delete in the address' datastore
    pub fn delete_data_entry(&mut self, addr: &Address, key: &[u8]) -> Result<(), ExecutionError> {
        // check if the entry exists
        if let Some(value) = self.get_data_entry(addr, key) {
            let value_storage_cost = self.get_storage_cost_datastore_value(&value)?;
            self.transfer_coins(
                None,
                Some(*addr),
                self.storage_costs_constants
                    .ledger_entry_datastore_base_cost
                    .checked_add(value_storage_cost)
                    .ok_or_else(|| {
                        ExecutionError::RuntimeError(
                            "overflow when calculating storage cost for datastore key/value"
                                .to_string(),
                        )
                    })?,
            )?;
        } else {
            return Err(ExecutionError::RuntimeError(format!(
                "could not delete data entry {:?} for address {}: entry does not exist",
                key, addr
            )));
        }

        // delete entry
        self.added_changes.delete_data_entry(*addr, key.to_owned());

        Ok(())
    }
}
