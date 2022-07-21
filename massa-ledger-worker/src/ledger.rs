// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final ledger associating addresses to their balances, bytecode and data.

use crate::ledger_db::{LedgerDB, LedgerSubEntry};
use massa_ledger_exports::{
    LedgerChanges, LedgerConfig, LedgerController, LedgerEntry, LedgerError,
};
use massa_models::{Address, Amount, ModelsError};
use massa_models::{DeserializeCompact, Slot};
use nom::AsBytes;
use std::collections::{BTreeSet, HashMap};

/// Represents a final ledger associating addresses to their balances, bytecode and data.
/// The final ledger is part of the final state which is attached to a final slot, can be bootstrapped and allows others to bootstrap.
/// The ledger size can be very high: it can exceed 1 terabyte.
/// To allow for storage on disk, the ledger uses trees and has `O(log(N))` access, insertion and deletion complexity.
#[derive(Debug)]
pub struct FinalLedger {
    /// ledger configuration
    pub(crate) _config: LedgerConfig,
    /// ledger tree, sorted by address
    pub(crate) sorted_ledger: LedgerDB,
}

/// Macro used to shorten file error returns
macro_rules! init_file_error {
    ($st:expr, $cfg:ident) => {
        |err| {
            LedgerError::FileError(format!(
                "error $st initial ledger file {}: {}",
                $cfg.initial_sce_ledger_path
                    .to_str()
                    .unwrap_or("(non-utf8 path)"),
                err
            ))
        }
    };
}
pub(crate) use init_file_error;

impl FinalLedger {
    /// Initializes a new `FinalLedger` by reading its initial state from file.
    pub fn new(config: LedgerConfig) -> Result<Self, LedgerError> {
        // load the ledger tree from file
        let initial_ledger: HashMap<Address, LedgerEntry> =
            serde_json::from_str::<HashMap<Address, Amount>>(
                &std::fs::read_to_string(&config.initial_sce_ledger_path)
                    .map_err(init_file_error!("loading", config))?,
            )
            .map_err(init_file_error!("parsing", config))?
            .into_iter()
            .map(|(addr, amount)| {
                (
                    addr,
                    LedgerEntry {
                        parallel_balance: amount,
                        ..Default::default()
                    },
                )
            })
            .collect();

        // create and initialize the disk ledger
        let mut sorted_ledger = LedgerDB::new(config.disk_ledger_path.clone());
        sorted_ledger.set_initial_ledger(initial_ledger);

        // generate the final ledger
        Ok(FinalLedger {
            sorted_ledger,
            _config: config,
        })
    }
}

impl LedgerController for FinalLedger {
    /// Allows applying `LedgerChanges` to the final ledger
    fn apply_changes(&mut self, changes: LedgerChanges, slot: Slot) {
        self.sorted_ledger.apply_changes(changes, slot);
    }

    /// Gets the parallel balance of a ledger entry
    ///
    /// # Returns
    /// The parallel balance, or None if the ledger entry was not found
    fn get_parallel_balance(&self, addr: &Address) -> Option<Amount> {
        self.sorted_ledger
            .get_sub_entry(addr, LedgerSubEntry::Balance)
            .map(|bytes| {
                Amount::from_bytes_compact(&bytes)
                    .expect("critical: invalid balance format")
                    .0
            })
    }

    /// Gets a copy of the bytecode of a ledger entry
    ///
    /// # Returns
    /// A copy of the found bytecode, or None if the ledger entry was not found
    fn get_bytecode(&self, addr: &Address) -> Option<Vec<u8>> {
        self.sorted_ledger
            .get_sub_entry(addr, LedgerSubEntry::Bytecode)
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

    /// Checks for the existence of a datastore entry for a given address.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    ///
    /// # Returns
    /// true if the datastore entry was found, or false if the ledger entry or datastore entry was not found
    fn has_data_entry(&self, addr: &Address, key: &[u8]) -> bool {
        self.sorted_ledger
            .get_sub_entry(addr, LedgerSubEntry::Datastore(key.to_owned()))
            .is_some()
    }

    /// Get every key of the datastore for a given address.
    ///
    /// # Returns
    /// A BTreeSet of the datastore keys
    fn get_datastore_keys(&self, addr: &Address) -> BTreeSet<Vec<u8>> {
        self.sorted_ledger.get_datastore_keys(addr)
    }

    /// Get a part of the disk ledger.
    ///
    /// Solely used by the bootstrap.
    ///
    /// # Returns
    /// A tuple containing the data and the last returned key
    fn get_ledger_part(
        &self,
        last_key: &Option<Vec<u8>>,
    ) -> Result<(Vec<u8>, Option<Vec<u8>>), ModelsError> {
        self.sorted_ledger.get_ledger_part(last_key)
    }

    /// Set a part of the disk ledger.
    ///
    /// Solely used by the bootstrap.
    ///
    /// # Returns
    /// The last key inserted
    fn set_ledger_part(&self, data: Vec<u8>) -> Result<Option<Vec<u8>>, ModelsError> {
        self.sorted_ledger.set_ledger_part(data.as_bytes())
    }

    /// Get every address and their corresponding balance.
    ///
    /// IMPORTANT: This should only be used for debug and test purposes.
    ///
    /// # Returns
    /// A BTreeMap with the address as key and the balance as value
    #[cfg(feature = "testing")]
    fn get_every_address(&self) -> std::collections::BTreeMap<Address, Amount> {
        self.sorted_ledger.get_every_address()
    }

    /// Get the entire datastore for a given address.
    ///
    /// IMPORTANT: This should only be used for debug purposes.
    ///
    /// # Returns
    /// A BTreeMap with the entry hash as key and the data bytes as value
    #[cfg(feature = "testing")]
    fn get_entire_datastore(&self, addr: &Address) -> std::collections::BTreeMap<Vec<u8>, Vec<u8>> {
        self.sorted_ledger.get_entire_datastore(addr)
    }
}
