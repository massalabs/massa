// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final ledger associating addresses to their balances, bytecode and data.

use crate::ledger_changes::LedgerChanges;
use crate::ledger_db::{LedgerDB, LedgerSubEntry};
use crate::ledger_entry::LedgerEntry;
use crate::types::{Applicable, SetUpdateOrDelete};
use crate::{FinalLedgerBootstrapState, LedgerConfig, LedgerError};
use massa_hash::Hash;
use massa_models::{Address, Amount};
use std::collections::BTreeMap;

/// Represents a final ledger associating addresses to their balances, bytecode and data.
/// The final ledger is part of the final state which is attached to a final slot, can be bootstrapped and allows others to bootstrap.
/// The ledger size can be very high: it can exceed 1 terabyte.
/// To allow for storage on disk, the ledger uses trees and has `O(log(N))` access, insertion and deletion complexity.
pub struct FinalLedger {
    /// ledger configuration
    _config: LedgerConfig,
    /// ledger tree, sorted by address
    sorted_ledger: LedgerDB,
}

/// Allows applying `LedgerChanges` to the final ledger
impl Applicable<LedgerChanges> for FinalLedger {
    fn apply(&mut self, changes: LedgerChanges) {
        // for all incoming changes
        for (addr, change) in changes.0 {
            match change {
                // the incoming change sets a ledger entry to a new one
                SetUpdateOrDelete::Set(new_entry) => {
                    // inserts/overwrites the entry with the incoming one
                    self.sorted_ledger.put(&addr, new_entry);
                }
                // the incoming change updates an existing ledger entry
                SetUpdateOrDelete::Update(entry_update) => {
                    // applies the updates to the entry
                    // if the entry does not exist, inserts a default one and applies the updates to it
                    self.sorted_ledger.update(&addr, entry_update);
                }
                // the incoming change deletes a ledger entry
                SetUpdateOrDelete::Delete => {
                    // delete the entry, if it exists
                    self.sorted_ledger.delete(&addr);
                }
            }
        }
    }
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
        // open db with default options
        // and use only single threaded mode
        // should explore these options before review
        let mut sorted_ledger = LedgerDB::new();

        // load the ledger tree from file
        let initial_ledger = serde_json::from_str::<BTreeMap<Address, Amount>>(
            &std::fs::read_to_string(&config.initial_sce_ledger_path)
                .map_err(init_file_error!("loading", config))?,
        )
        .map_err(init_file_error!("parsing", config))?;

        // parsing from json to rust types, then types to bytes, maybe
        // there is a better way to do this
        for (address, amount) in &initial_ledger {
            sorted_ledger.put(
                address,
                LedgerEntry {
                    parallel_balance: *amount,
                    ..Default::default()
                },
            );
        }

        // generate the final ledger
        Ok(FinalLedger {
            sorted_ledger,
            _config: config,
        })
    }

    /// Initialize a `FinalLedger` from a bootstrap state
    ///
    /// TODO: This loads the whole ledger in RAM. Switch to streaming in the future
    ///
    /// # Arguments
    /// * configuration: ledger configuration
    /// * state: bootstrap state
    pub fn from_bootstrap_state(config: LedgerConfig, _state: FinalLedgerBootstrapState) -> Self {
        // dummy implementation since bootstrap streaming is currently developed
        FinalLedger {
            sorted_ledger: LedgerDB::new(),
            _config: config,
        }
    }

    /// Gets a snapshot of the ledger to bootstrap other nodes
    ///
    /// TODO: This loads the whole ledger in RAM. Switch to streaming in the future
    pub fn get_bootstrap_state(&self) -> FinalLedgerBootstrapState {
        // dummy implementation since bootstrap streaming is currently developed
        FinalLedgerBootstrapState {
            sorted_ledger: BTreeMap::new(),
        }
    }

    /// Gets the parallel balance of a ledger entry
    ///
    /// # Returns
    /// The parallel balance, or None if the ledger entry was not found
    pub fn get_parallel_balance(&self, addr: &Address) -> Option<Amount> {
        // note: can a balance ever have a invalid format?
        self.sorted_ledger
            .get_entry(addr, LedgerSubEntry::Balance)
            .map(|bytes| {
                Amount::from_raw(u64::from_be_bytes(
                    bytes.try_into().expect("critical: invalid balance format"),
                ))
            })
    }

    /// Gets a copy of the bytecode of a ledger entry
    ///
    /// # Returns
    /// A copy of the found bytecode, or None if the ledger entry was not found
    pub fn get_bytecode(&self, addr: &Address) -> Option<Vec<u8>> {
        self.sorted_ledger.get_entry(addr, LedgerSubEntry::Bytecode)
    }

    /// Checks if a ledger entry exists
    ///
    /// # Returns
    /// true if it exists, false otherwise.
    pub fn entry_exists(&self, addr: &Address) -> bool {
        // note: document the "may"
        self.sorted_ledger
            .entry_may_exist(addr, LedgerSubEntry::Balance)
    }

    /// Gets a copy of the value of a datastore entry for a given address.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    ///
    /// # Returns
    /// A copy of the datastore value, or `None` if the ledger entry or datastore entry was not found
    pub fn get_data_entry(&self, addr: &Address, key: &Hash) -> Option<Vec<u8>> {
        self.sorted_ledger
            .get_entry(addr, LedgerSubEntry::Datastore(*key))
    }

    /// Checks for the existence of a datastore entry for a given address.
    ///
    /// # Arguments
    /// * `addr`: target address
    /// * `key`: datastore key
    ///
    /// # Returns
    /// true if the datastore entry was found, or false if the ledger entry or datastore entry was not found
    pub fn has_data_entry(&self, addr: &Address, key: &Hash) -> bool {
        // note: document the "may"
        self.sorted_ledger
            .entry_may_exist(addr, LedgerSubEntry::Datastore(*key))
    }
}
