// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final ledger associating addresses to their balances, bytecode and data.

use crate::ledger_changes::LedgerChanges;
use crate::ledger_entry::LedgerEntry;
use crate::types::{Applicable, SetUpdateOrDelete};
use crate::{FinalLedgerBootstrapState, LedgerConfig, LedgerError};
use massa_hash::Hash;
use massa_models::{Address, Amount};
use rocksdb::DB;
use std::collections::BTreeMap;

const BINCODE_ERROR: &str = "critical: internal bincode operation failed";
const ROCKSDB_ERROR: &str = "critical: rocksdb crud operation failed";

/// Represents a final ledger associating addresses to their balances, bytecode and data.
/// The final ledger is part of the final state which is attached to a final slot, can be bootstrapped and allows others to bootstrap.
/// The ledger size can be very high: it can exceed 1 terabyte.
/// To allow for storage on disk, the ledger uses trees and has `O(log(N))` access, insertion and deletion complexity.
///
/// Note: currently the ledger is stored in RAM. TODO put it on the hard drive with cache.
pub struct FinalLedger {
    /// ledger configuration
    _config: LedgerConfig,
    /// ledger tree, sorted by address
    sorted_ledger: DB,
    // sorted_ledger: BTreeMap<Address, LedgerEntry>,
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
                    self.sorted_ledger
                        .put(
                            addr.to_bytes(),
                            bincode::serialize(&new_entry).expect(BINCODE_ERROR),
                        )
                        .expect(ROCKSDB_ERROR);
                }

                // the incoming change updates an existing ledger entry
                SetUpdateOrDelete::Update(_entry_update) => {
                    // applies the updates to the entry
                    // if the entry does not exist, inserts a default one and applies the updates to it
                    match self
                        .sorted_ledger
                        .get(addr.to_bytes())
                        .expect(ROCKSDB_ERROR)
                    {
                        // important: this requires 1 deser 1 apply 1 ser
                        // TODO: find a way to perform only 1 apply
                        // TODO: save each address entry on 1 different key
                        Some(_bytes) => self
                            .sorted_ledger
                            .put(
                                addr.to_bytes(),
                                b"",
                                // bincode::deserialize::<LedgerEntry>(&bytes)
                                //     .expect(BINCODE_ERROR)
                                //     .apply(entry_update),
                            )
                            .expect(ROCKSDB_ERROR),
                        None => self
                            .sorted_ledger
                            .put(addr.to_bytes(), b"")
                            .expect(ROCKSDB_ERROR), // create new default entry and updates it
                    };
                    // self.sorted_ledger
                    //     .get(addr)
                    //     .unwrap()
                    //     .or_insert_with(Default::default)
                    //     .apply(entry_update);
                }

                // the incoming change deletes a ledger entry
                SetUpdateOrDelete::Delete => {
                    // delete the entry, if it exists
                    self.sorted_ledger
                        .delete(addr.to_bytes())
                        .expect(ROCKSDB_ERROR);
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
        let sorted_ledger = DB::open_default("_path_for_rocksdb_storage").unwrap();

        // load the ledger tree from file
        let initial_ledger = serde_json::from_str::<BTreeMap<Address, Amount>>(
            &std::fs::read_to_string(&config.initial_sce_ledger_path)
                .map_err(init_file_error!("loading", config))?,
        )
        .map_err(init_file_error!("parsing", config))?;

        // parsing from json to rust types, then types to bytes, maybe
        // there is a better way to do this
        for (address, amount) in &initial_ledger {
            sorted_ledger
                .put(
                    address.to_bytes(),
                    bincode::serialize(&LedgerEntry {
                        parallel_balance: *amount,
                        ..Default::default()
                    })
                    .map_err(init_file_error!("serialize", config))?,
                )
                .map_err(init_file_error!("insert", config))?;
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
        // conversion code if needed for testing
        // temporary because bootstrap streaming is not ready yet
        // let sorted_ledger = DB::open_default("_path_for_rocksdb_storage").unwrap();
        // for (address, entry) in &state.sorted_ledger {
        //     sorted_ledger
        //         .put(
        //             address.to_bytes(),
        //             bincode::serialize(entry).expect("WIIL BE NATURALLY REMOVED"),
        //         )
        //         .expect("WIIL BE NATURALLY REMOVED");
        // }
        // dummy temp implem since an actual one would have no purpose rn
        FinalLedger {
            sorted_ledger: DB::open_default("_path_for_rocksdb_storage").unwrap(),
            _config: config,
        }
    }

    /// Gets a snapshot of the ledger to bootstrap other nodes
    ///
    /// TODO: This loads the whole ledger in RAM. Switch to streaming in the future
    pub fn get_bootstrap_state(&self) -> FinalLedgerBootstrapState {
        // dummy temp implem since an actual one would have no purpose rn
        FinalLedgerBootstrapState {
            sorted_ledger: BTreeMap::new(),
        }
    }

    /// Gets a copy of a full ledger entry.
    ///
    /// # Returns
    /// A clone of the whole `LedgerEntry`, or None if not found.
    ///
    /// TODO: in the future, never manipulate full ledger entries because their datastore can be huge
    /// `https://github.com/massalabs/massa/issues/2342`
    pub fn get_full_entry(&self, addr: &Address) -> Option<LedgerEntry> {
        // would probably be worth it to make a db module
        self.sorted_ledger
            .get(addr.to_bytes())
            .ok()
            .flatten()
            .map(|bytes| bincode::deserialize(&bytes).expect("HANDLE THIS"))
    }

    /// Gets the parallel balance of a ledger entry
    ///
    /// # Returns
    /// The parallel balance, or None if the ledger entry was not found
    pub fn get_parallel_balance(&self, addr: &Address) -> Option<Amount> {
        self.sorted_ledger
            .get(addr.to_bytes())
            .expect(ROCKSDB_ERROR)
            .map(|bytes| {
                bincode::deserialize::<LedgerEntry>(&bytes)
                    .expect(BINCODE_ERROR)
                    .parallel_balance
            })
    }

    /// Gets a copy of the bytecode of a ledger entry
    ///
    /// # Returns
    /// A copy of the found bytecode, or None if the ledger entry was not found
    pub fn get_bytecode(&self, addr: &Address) -> Option<Vec<u8>> {
        self.sorted_ledger
            .get(addr.to_bytes())
            .expect(ROCKSDB_ERROR)
            .map(|bytes| {
                bincode::deserialize::<LedgerEntry>(&bytes)
                    .expect(BINCODE_ERROR)
                    .bytecode
            })
    }

    /// Checks if a ledger entry exists
    ///
    /// # Returns
    /// true if it exists, false otherwise.
    pub fn entry_exists(&self, addr: &Address) -> bool {
        self.sorted_ledger.key_may_exist(addr.to_bytes())
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
            .get(addr.to_bytes())
            .expect(ROCKSDB_ERROR)
            .map(|bytes| {
                bincode::deserialize::<LedgerEntry>(&bytes)
                    .expect(BINCODE_ERROR)
                    .datastore
                    .get(key)
                    .cloned()
                    .expect("missing datastore key")
            })
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
        self.sorted_ledger
            .get(addr.to_bytes())
            .expect(ROCKSDB_ERROR)
            .map(|bytes| {
                bincode::deserialize::<LedgerEntry>(&bytes)
                    .expect(BINCODE_ERROR)
                    .datastore
                    .contains_key(key)
            })
            .expect("missing ledger key")
    }
}
