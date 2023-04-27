// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final ledger associating addresses to their balances, bytecode and data.

use crate::ledger_db::{LedgerDB, LedgerSubEntry};
use massa_hash::Hash;
use massa_ledger_exports::{
    Key, LedgerChanges, LedgerConfig, LedgerController, LedgerEntry, LedgerError,
};
use massa_models::{
    address::Address,
    amount::{Amount, AmountDeserializer},
    bytecode::{Bytecode, BytecodeDeserializer},
    error::ModelsError,
    slot::Slot,
    streaming_step::StreamingStep,
};
use massa_serialization::{DeserializeError, Deserializer};
use nom::AsBytes;
use std::collections::{BTreeSet, HashMap};
use std::ops::Bound::Included;

/// Represents a final ledger associating addresses to their balances, bytecode and data.
/// The final ledger is part of the final state which is attached to a final slot, can be bootstrapped and allows others to bootstrap.
/// The ledger size can be very high: it can exceed 1 terabyte.
/// To allow for storage on disk, the ledger uses trees and has `O(log(N))` access, insertion and deletion complexity.
#[derive(Debug)]
pub struct FinalLedger {
    /// ledger configuration
    pub(crate) config: LedgerConfig,
    /// ledger tree, sorted by address
    pub(crate) sorted_ledger: LedgerDB,
}

impl FinalLedger {
    /// Initializes a new `FinalLedger` by reading its initial state from file.
    pub fn new(config: LedgerConfig, with_final_state: bool) -> Self {
        // create and initialize the disk ledger
        let sorted_ledger = LedgerDB::new(
            config.disk_ledger_path.clone(),
            config.thread_count,
            config.max_key_length,
            config.max_ledger_part_size,
            with_final_state,
        );

        // generate the final ledger
        FinalLedger {
            sorted_ledger,
            config,
        }
    }
}

impl LedgerController for FinalLedger {
    /// Allows applying `LedgerChanges` to the final ledger
    fn apply_changes(
        &mut self,
        changes: LedgerChanges,
        slot: Slot,
        final_state_data: Option<Vec<u8>>,
    ) {
        self.sorted_ledger
            .apply_changes(changes, slot, final_state_data);
    }

    /// Loads ledger from file
    fn load_initial_ledger(&mut self) -> Result<(), LedgerError> {
        // load the ledger tree from file
        let initial_ledger: HashMap<Address, LedgerEntry> = serde_json::from_str(
            &std::fs::read_to_string(&self.config.initial_ledger_path).map_err(|err| {
                LedgerError::FileError(format!(
                    "error loading initial ledger file {}: {}",
                    self.config
                        .initial_ledger_path
                        .to_str()
                        .unwrap_or("(non-utf8 path)"),
                    err
                ))
            })?,
        )
        .map_err(|err| {
            LedgerError::FileError(format!(
                "error parsing initial ledger file {}: {}",
                self.config
                    .initial_ledger_path
                    .to_str()
                    .unwrap_or("(non-utf8 path)"),
                err
            ))
        })?;
        self.sorted_ledger.load_initial_ledger(initial_ledger);
        Ok(())
    }

    /// Gets the balance of a ledger entry
    ///
    /// # Returns
    /// The balance, or None if the ledger entry was not found
    fn get_balance(&self, addr: &Address) -> Option<Amount> {
        let amount_deserializer =
            AmountDeserializer::new(Included(Amount::MIN), Included(Amount::MAX));
        self.sorted_ledger
            .get_sub_entry(addr, LedgerSubEntry::Balance)
            .map(|bytes| {
                amount_deserializer
                    .deserialize::<DeserializeError>(&bytes)
                    .expect("critical: invalid balance format")
                    .1
            })
    }

    /// Gets a copy of the bytecode of a ledger entry
    ///
    /// # Returns
    /// A copy of the found bytecode, or None if the ledger entry was not found
    fn get_bytecode(&self, addr: &Address) -> Option<Bytecode> {
        let bytecode_deserializer =
            BytecodeDeserializer::new(self.config.max_datastore_value_length);
        self.sorted_ledger
            .get_sub_entry(addr, LedgerSubEntry::Bytecode)
            .map(|bytes| {
                bytecode_deserializer
                    .deserialize::<DeserializeError>(&bytes)
                    .expect("critical: invalid bytecode format")
                    .1
            })
    }

    /// Checks if a ledger entry exists
    ///
    /// # Returns
    /// true if it exists, false otherwise.
    fn entry_exists(&self, addr: &Address) -> bool {
        self.sorted_ledger
            .get_sub_entry(addr, LedgerSubEntry::Balance)
            .is_some()
    }

    /// Gets a copy of the value of a datastore entry for a given address.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    ///
    /// # Returns
    /// A copy of the datastore value, or `None` if the ledger entry or datastore entry was not found
    fn get_data_entry(&self, addr: &Address, key: &[u8]) -> Option<Vec<u8>> {
        self.sorted_ledger
            .get_sub_entry(addr, LedgerSubEntry::Datastore(key.to_owned()))
    }

    /// Get every key of the datastore for a given address.
    ///
    /// # Returns
    /// A `BTreeSet` of the datastore keys
    fn get_datastore_keys(&self, addr: &Address) -> Option<BTreeSet<Vec<u8>>> {
        self.sorted_ledger.get_datastore_keys(addr)
    }

    /// Get the current disk ledger hash
    fn get_ledger_hash(&self) -> Hash {
        self.sorted_ledger.get_ledger_hash()
    }

    /// Get a part of the disk ledger.
    ///
    /// Solely used by the bootstrap.
    ///
    /// # Returns
    /// A tuple containing the data and the last returned key
    fn get_ledger_part(
        &self,
        last_key: StreamingStep<Key>,
    ) -> Result<(Vec<u8>, StreamingStep<Key>), ModelsError> {
        self.sorted_ledger.get_ledger_part(last_key)
    }

    /// Set a part of the disk ledger.
    ///
    /// Solely used by the bootstrap.
    ///
    /// # Returns
    /// The last key inserted
    fn set_ledger_part(&self, data: Vec<u8>) -> Result<StreamingStep<Key>, ModelsError> {
        self.sorted_ledger.set_ledger_part(data.as_bytes())
    }

    /// Reset the disk ledger.
    ///
    /// USED FOR BOOTSTRAP ONLY
    fn reset(&mut self) {
        self.sorted_ledger.reset();
    }

    fn set_initial_slot(&mut self, slot: Slot) {
        self.sorted_ledger.set_initial_slot(slot);
    }

    /// Get the slot associated with the current ledger
    fn get_slot(&self) -> Result<Slot, ModelsError> {
        self.sorted_ledger.get_slot()
    }

    /// Set the final_state_hash of the slot associated with the current ledger
    /// Can be used to verify the integrity of the final state saved when restarting from snapshot
    fn set_final_state_hash(&mut self, data: Vec<u8>) {
        self.sorted_ledger.set_final_state_hash(&data)
    }

    /// Get the final state stored in the ledger, to restart from snapshot
    fn get_final_state(&self) -> Result<Vec<u8>, ModelsError> {
        self.sorted_ledger.get_final_state()
    }

    /// Get every address and their corresponding balance.
    ///
    /// IMPORTANT: This should only be used for debug and test purposes.
    ///
    /// # Returns
    /// A `BTreeMap` with the address as key and the balance as value
    #[cfg(feature = "testing")]
    fn get_every_address(&self) -> std::collections::BTreeMap<Address, Amount> {
        self.sorted_ledger.get_every_address()
    }

    /// Get the entire datastore for a given address.
    ///
    /// IMPORTANT: This should only be used for debug purposes.
    ///
    /// # Returns
    /// A `BTreeMap` with the entry hash as key and the data bytes as value
    #[cfg(feature = "testing")]
    fn get_entire_datastore(&self, addr: &Address) -> std::collections::BTreeMap<Vec<u8>, Vec<u8>> {
        self.sorted_ledger.get_entire_datastore(addr)
    }
}
