// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative ledger represents, in a compressed way,
//! the state of the ledger at an arbitrary execution slot.
//! It never actually writes to the consensus state
//! but keeps track of the changes that were applied to it since its creation.

use crate::active_history::{ActiveHistory, HistorySearchResult};
use massa_execution_exports::ExecutionError;
use massa_execution_exports::StorageCostsConstants;
use massa_final_state::FinalStateController;
use massa_ledger_exports::{Applicable, LedgerChanges, SetOrDelete, SetUpdateOrDelete};
use massa_models::bytecode::Bytecode;
use massa_models::datastore::get_prefix_bounds;
use massa_models::{address::Address, amount::Amount};
use parking_lot::RwLock;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::Arc;
use tracing::debug;

/// The `SpeculativeLedger` contains an thread-safe shared reference to the final ledger (read-only),
/// a list of existing changes that happened o the ledger since its finality,
/// as well as an extra list of "added" changes.
/// The `SpeculativeLedger` makes it possible to transparently manipulate a virtual ledger
/// that takes into account all those ledger changes and allows adding more
/// while keeping track of all the newly added changes, and never writing in the final ledger.
pub(crate) struct SpeculativeLedger {
    /// Thread-safe shared access to the final state. For reading only.
    final_state: Arc<RwLock<dyn FinalStateController>>,

    /// History of the outputs of recently executed slots.
    /// Slots should be consecutive, newest at the back.
    active_history: Arc<RwLock<ActiveHistory>>,

    /// list of ledger changes that were applied to this `SpeculativeLedger` since its creation
    pub added_changes: LedgerChanges,

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
        final_state: Arc<RwLock<dyn FinalStateController>>,
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
                HistorySearchResult::NoInfo => {
                    self.final_state.read().get_ledger().get_balance(addr)
                }
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
    /// `Some(Bytecode)` if the address was found, otherwise None
    pub fn get_bytecode(&self, addr: &Address) -> Option<Bytecode> {
        // try to read from added changes > history > final_state
        self.added_changes.get_bytecode_or_else(addr, || {
            match self.active_history.read().fetch_bytecode(addr) {
                HistorySearchResult::Present(bytecode) => Some(bytecode),
                HistorySearchResult::NoInfo => {
                    self.final_state.read().get_ledger().get_bytecode(addr)
                }
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
                .ok_or_else(|| ExecutionError::RuntimeError(format!("spending address {} not found", from_addr)))?
                .checked_sub(amount)
                .ok_or_else(|| {
                    ExecutionError::RuntimeError(format!("failed to transfer {} coins from spending address {} due to insufficient balance {}", amount, from_addr, self
                    .get_balance(&from_addr).unwrap_or_default()))
                })?;

            // update the balance of the sender address
            changes.set_balance(from_addr, new_balance);
        }

        // simulate crediting coins to destination address (if any)
        // note that to_addr can be the same as from_addr
        if let Some(to_addr) = to_addr {
            if let Some(old_balance) =
                changes.get_balance_or_else(&to_addr, || self.get_balance(&to_addr))
            {
                // if `to_addr` exists we increase its balance
                let new_balance = old_balance.checked_add(amount).ok_or_else(|| {
                    ExecutionError::RuntimeError(format!(
                        "overflow in crediting address {} balance {} due to adding {} coins to balance",
                        to_addr, old_balance, amount
                    ))
                })?;
                changes.set_balance(to_addr, new_balance);
            } else if let Some(remaining_coins) =
                amount.checked_sub(self.storage_costs_constants.ledger_entry_base_cost)
            {
                // if `to_addr` doesn't exist and we have enough coins to credit it,
                // we will try to create the address using part of the transferred coins
                debug!("Creating address {} from coins", to_addr);
                changes.create_address(&to_addr);
                changes.set_balance(to_addr, remaining_coins);
            } else {
                // `to_addr` does not exist and we don't have the money to create it
                return Err(ExecutionError::RuntimeError(format!(
                    "insufficient amount {} to create credited address {}",
                    amount, to_addr
                )));
            }
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
                HistorySearchResult::NoInfo => {
                    self.final_state.read().get_ledger().entry_exists(addr)
                }
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
        bytecode: Bytecode,
    ) -> Result<(), ExecutionError> {
        // check for address existence
        if !self.entry_exists(&creator_address) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not create SC address {}: creator address {} does not exist",
                addr, creator_address
            )));
        }

        // check that we don't collide with existing address
        if self.entry_exists(&addr) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not create SC address {}: target address already exists",
                addr
            )));
        }

        if bytecode.0.len() > self.max_bytecode_size as usize {
            return Err(ExecutionError::RuntimeError(format!(
                "could not create SC address {}: bytecode size exceeds maximum allowed size",
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
                    .checked_mul_u64(bytecode.0.len().try_into().map_err(|_| {
                        ExecutionError::RuntimeError(
                            "overflow while calculating bytecode ledger size costs".to_string(),
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
        self.added_changes.create_address(&addr);
        self.added_changes.set_bytecode(addr, bytecode);
        Ok(())
    }

    /// Sets the bytecode associated to an address in the ledger.
    /// Fails if the address doesn't exist.
    ///
    /// # Arguments
    /// * `caller_addr`: address of the caller. Will pay the storage costs.
    /// * `addr`: target address
    /// * `bytecode`: bytecode to set for that address
    pub fn set_bytecode(
        &mut self,
        caller_addr: &Address,
        addr: &Address,
        bytecode: Bytecode,
    ) -> Result<(), ExecutionError> {
        // check for address existence
        if !self.entry_exists(addr) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set bytecode for address {}: address does not exist",
                addr
            )));
        }

        if bytecode.0.len() > self.max_bytecode_size as usize {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set bytecode for address {}: bytecode size exceeds maximum allowed size",
                addr
            )));
        }

        if let Some(old_bytecode_size) = self.get_bytecode(addr).map(|b| b.0.len()) {
            let diff_size_storage: i64 = (bytecode.0.len() as i64) - (old_bytecode_size as i64);
            let storage_cost_bytecode = self
                .storage_costs_constants
                .ledger_cost_per_byte
                .checked_mul_u64(diff_size_storage.unsigned_abs())
                .ok_or_else(|| {
                    ExecutionError::RuntimeError(
                        "overflow on computing bytecode delta storage costs".to_string(),
                    )
                })?;

            match diff_size_storage.signum() {
                1 => self.transfer_coins(Some(*caller_addr), None, storage_cost_bytecode)?,
                -1 => self.transfer_coins(None, Some(*caller_addr), storage_cost_bytecode)?,
                _ => {}
            };
        } else {
            let bytecode_storage_cost = self
                .storage_costs_constants
                .ledger_cost_per_byte
                .checked_mul_u64(bytecode.0.len() as u64)
                .ok_or_else(|| {
                    ExecutionError::RuntimeError(
                        "overflow when calculating storage cost of bytecode".to_string(),
                    )
                })?;
            self.transfer_coins(Some(*caller_addr), None, bytecode_storage_cost)?;
        }
        // set the bytecode of that address
        self.added_changes.set_bytecode(*addr, bytecode);

        Ok(())
    }

    /// Gets a copy of a datastore keys for a given address
    ///
    /// # Arguments
    /// * `addr`: address to query
    /// * `prefix`: prefix to filter the keys
    ///
    /// # Returns
    /// `Some(Vec<Vec<u8>>)` for found keys, `None` if the address does not exist.
    pub fn get_keys(&self, addr: &Address, prefix: &[u8]) -> Option<BTreeSet<Vec<u8>>> {
        // compute prefix range
        let prefix_range = get_prefix_bounds(prefix);
        let range_ref = (prefix_range.0.as_ref(), prefix_range.1.as_ref());

        // init keys with final state
        let mut candidate_keys: Option<BTreeSet<Vec<u8>>> = self
            .final_state
            .read()
            .get_ledger()
            .get_datastore_keys(addr, prefix);

        // here, traverse the history from oldest to newest with added_changes at the end, applying additions and deletions
        let active_history = self.active_history.read();
        let changes_iterator = active_history
            .0
            .iter()
            .map(|item| &item.state_changes.ledger_changes)
            .chain(std::iter::once(&self.added_changes));
        for ledger_changes in changes_iterator {
            match ledger_changes.get(addr) {
                // address absent from the changes
                None => (),

                // address ledger entry being reset to an absolute new list of keys
                Some(SetUpdateOrDelete::Set(new_ledger_entry)) => {
                    candidate_keys = Some(
                        new_ledger_entry
                            .datastore
                            .range::<Vec<u8>, _>(range_ref)
                            .map(|(k, _v)| k.clone())
                            .collect(),
                    );
                }

                // address ledger entry being updated
                Some(SetUpdateOrDelete::Update(entry_updates)) => {
                    let c_k = candidate_keys.get_or_insert_with(Default::default);
                    for (ds_key, ds_update) in
                        entry_updates.datastore.range::<Vec<u8>, _>(range_ref)
                    {
                        match ds_update {
                            SetOrDelete::Set(_) => c_k.insert(ds_key.clone()),
                            SetOrDelete::Delete => c_k.remove(ds_key),
                        };
                    }
                }

                // address ledger entry being deleted
                Some(SetUpdateOrDelete::Delete) => {
                    candidate_keys = None;
                }
            }
        }

        candidate_keys
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
                HistorySearchResult::NoInfo => self
                    .final_state
                    .read()
                    .get_ledger()
                    .get_data_entry(addr, key),
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
                HistorySearchResult::NoInfo => self
                    .final_state
                    .read()
                    .get_ledger()
                    .get_data_entry(addr, key)
                    .is_some(),
                HistorySearchResult::Absent => false,
            }
        })
    }

    /// Compute the storage costs of a full datastore entry
    fn get_storage_cost_datastore_entry(
        &self,
        key: &[u8],
        value: &[u8],
    ) -> Result<Amount, ExecutionError> {
        // Base cost, charged for each datastore entry.
        // This accounts for small entries that still take space in practice.
        let base_cost = self
            .storage_costs_constants
            .ledger_entry_datastore_base_cost;

        // key cost
        let key_cost = self
            .storage_costs_constants
            .ledger_cost_per_byte
            .checked_mul_u64(key.len().try_into().map_err(|_| {
                ExecutionError::RuntimeError("key in datastore is too big".to_string())
            })?)
            .ok_or_else(|| {
                ExecutionError::RuntimeError(
                    "overflow when calculating storage cost for datastore key".to_string(),
                )
            })?;

        // value cost
        let value_cost = self
            .storage_costs_constants
            .ledger_cost_per_byte
            .checked_mul_u64(value.len().try_into().map_err(|_| {
                ExecutionError::RuntimeError("value in datastore is too big".to_string())
            })?)
            .ok_or_else(|| {
                ExecutionError::RuntimeError(
                    "overflow when calculating storage cost for datastore value".to_string(),
                )
            })?;

        // total cost
        base_cost
            .checked_add(key_cost)
            .and_then(|c| c.checked_add(value_cost))
            .ok_or_else(|| {
                ExecutionError::RuntimeError(
                    "overflow when calculating storage cost for datastore key/value".to_string(),
                )
            })
    }

    /// Charge the storage costs of a datastore entry change, if any.
    fn charge_datastore_entry_change_storage(
        &mut self,
        caller_addr: &Address,
        old_key_value: Option<(&[u8], &[u8])>,
        new_key_value: Option<(&[u8], &[u8])>,
    ) -> Result<(), ExecutionError> {
        // compute the old storage cost of the entry
        let old_storage_cost = old_key_value.map_or_else(
            || Ok(Amount::zero()),
            |(old_key, old_value)| self.get_storage_cost_datastore_entry(old_key, old_value),
        )?;

        // compute the new storage cost of the entry
        let new_storage_cost = new_key_value.map_or_else(
            || Ok(Amount::zero()),
            |(new_key, new_value)| self.get_storage_cost_datastore_entry(new_key, new_value),
        )?;

        // charge the difference
        match new_storage_cost.cmp(&old_storage_cost) {
            Ordering::Greater => {
                // more bytes are now occupied
                self.transfer_coins(
                    Some(*caller_addr),
                    None,
                    new_storage_cost.saturating_sub(old_storage_cost),
                )
            }
            Ordering::Less => {
                // some bytes have been freed
                self.transfer_coins(
                    None,
                    Some(*caller_addr),
                    old_storage_cost.saturating_sub(new_storage_cost),
                )
            }
            Ordering::Equal => {
                // no change
                Ok(())
            }
        }
        .map_err(|err| {
            ExecutionError::RuntimeError(format!(
                "failed to charge storage costs for datastore entry change: {}",
                err
            ))
        })
    }

    /// Sets a data set entry for a given address in the ledger.
    /// Fails if the address doesn't exist.
    /// If the datastore entry does not exist, it is created.
    ///
    /// # Arguments
    /// * `caller_addr`: address of the caller. Will pay the storage costs.
    /// * `addr`: target address
    /// * `key`: datastore key
    /// * `data`: value to associate to the datastore key
    pub fn set_data_entry(
        &mut self,
        caller_addr: &Address,
        addr: &Address,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<(), ExecutionError> {
        // check for address existence
        if !self.entry_exists(addr) {
            return Err(ExecutionError::RuntimeError(format!(
                "could not set data for address {}: address does not exist",
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

        // charge the storage costs of the entry change
        {
            let prev_value = self.get_data_entry(addr, &key);
            self.charge_datastore_entry_change_storage(
                caller_addr,
                prev_value.as_ref().map(|v| (&key[..], &v[..])),
                Some((&key, &value)),
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
    /// * `caller_addr`: address of the caller. Will pay the storage costs.
    /// * `addr`: address
    /// * `key`: key of the entry to delete in the address' datastore
    pub fn delete_data_entry(
        &mut self,
        caller_addr: &Address,
        addr: &Address,
        key: &[u8],
    ) -> Result<(), ExecutionError> {
        // check if the entry exists
        if let Some(value) = self.get_data_entry(addr, key) {
            // reimburse the storage costs of the entry
            self.charge_datastore_entry_change_storage(caller_addr, Some((key, &value)), None)?;
        } else {
            return Err(ExecutionError::RuntimeError(format!(
                "could not delete data entry {:?} for address {}: entry or address does not exist",
                key, addr
            )));
        }

        // delete entry
        self.added_changes.delete_data_entry(*addr, key.to_owned());

        Ok(())
    }
}
