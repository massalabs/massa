// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final ledger associating addresses to their balances, bytecode and data.

use crate::cursor::LedgerCursorStep;
use crate::ledger_changes::LedgerChanges;
use crate::ledger_entry::LedgerEntry;
use crate::types::{Applicable, SetUpdateOrDelete};
use crate::{FinalLedgerBootstrapState, LedgerConfig, LedgerCursor, LedgerError};
use massa_hash::Hash;
use massa_models::amount::{AmountDeserializer, AmountSerializer};
use massa_models::constants::default::MAXIMUM_BYTES_MESSAGE_BOOTSTRAP;
use massa_models::constants::ADDRESS_SIZE_BYTES;
use massa_models::Serializer;
use massa_models::{
    array_from_slice, Address, Amount, DeserializeCompact, Deserializer, ModelsError,
    SerializeCompact,
};
use massa_models::{DeserializeVarInt, SerializeVarInt};
use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};

/// Represents a final ledger associating addresses to their balances, bytecode and data.
/// The final ledger is part of the final state which is attached to a final slot, can be bootstrapped and allows others to bootstrap.
/// The ledger size can be very high: it can exceed 1 terabyte.
/// To allow for storage on disk, the ledger uses trees and has `O(log(N))` access, insertion and deletion complexity.
///
/// Note: currently the ledger is stored in RAM. TODO put it on the hard drive with cache.
#[derive(Debug)]
pub struct FinalLedger {
    /// ledger configuration
    _config: LedgerConfig,
    /// ledger tree, sorted by address
    sorted_ledger: BTreeMap<Address, LedgerEntry>,
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
                    self.sorted_ledger.insert(addr, new_entry);
                }

                // the incoming change updates an existing ledger entry
                SetUpdateOrDelete::Update(entry_update) => {
                    // applies the updates to the entry
                    // if the entry does not exist, inserts a default one and applies the updates to it
                    self.sorted_ledger
                        .entry(addr)
                        .or_insert_with(Default::default)
                        .apply(entry_update);
                }

                // the incoming change deletes a ledger entry
                SetUpdateOrDelete::Delete => {
                    // delete the entry, if it exists
                    self.sorted_ledger.remove(&addr);
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
        // load the ledger tree from file
        let sorted_ledger = serde_json::from_str::<BTreeMap<Address, Amount>>(
            &std::fs::read_to_string(&config.initial_sce_ledger_path)
                .map_err(init_file_error!("loading", config))?,
        )
        .map_err(init_file_error!("parsing", config))?
        .into_iter()
        .map(|(address, balance)| {
            (
                address,
                LedgerEntry {
                    parallel_balance: balance,
                    ..Default::default()
                },
            )
        })
        .collect();

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
    pub fn from_bootstrap_state(config: LedgerConfig, state: FinalLedgerBootstrapState) -> Self {
        FinalLedger {
            sorted_ledger: state.sorted_ledger,
            _config: config,
        }
    }

    /// Gets a snapshot of the ledger to bootstrap other nodes
    ///
    /// TODO: This loads the whole ledger in RAM. Switch to streaming in the future
    pub fn get_bootstrap_state(&self) -> FinalLedgerBootstrapState {
        FinalLedgerBootstrapState {
            sorted_ledger: self.sorted_ledger.clone(),
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
        self.sorted_ledger.get(addr).cloned()
    }

    /// Gets the parallel balance of a ledger entry
    ///
    /// # Returns
    /// The parallel balance, or None if the ledger entry was not found
    pub fn get_parallel_balance(&self, addr: &Address) -> Option<Amount> {
        self.sorted_ledger.get(addr).map(|v| v.parallel_balance)
    }

    /// Gets a copy of the bytecode of a ledger entry
    ///
    /// # Returns
    /// A copy of the found bytecode, or None if the ledger entry was not found
    pub fn get_bytecode(&self, addr: &Address) -> Option<Vec<u8>> {
        self.sorted_ledger.get(addr).map(|v| v.bytecode.clone())
    }

    /// Checks if a ledger entry exists
    ///
    /// # Returns
    /// true if it exists, false otherwise.
    pub fn entry_exists(&self, addr: &Address) -> bool {
        self.sorted_ledger.contains_key(addr)
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
            .get(addr)
            .and_then(|v| v.datastore.get(key).cloned())
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
            .get(addr)
            .map_or(false, |v| v.datastore.contains_key(key))
    }
}

/// Part of the ledger of execution
#[derive(Debug, Clone)]
pub struct ExecutionLedgerSubset(pub BTreeMap<Address, LedgerEntry>);

impl SerializeCompact for ExecutionLedgerSubset {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        let entry_count: u64 = self.0.len().try_into().map_err(|err| {
            massa_models::ModelsError::SerializeError(format!(
                "too many entries in ConsensusLedgerSubset: {}",
                err
            ))
        })?;
        res.extend(entry_count.to_varint_bytes());
        for (address, data) in self.0.iter() {
            res.extend(&address.to_bytes());
            res.extend(&data.to_bytes_compact()?);
        }

        Ok(res)
    }
}

impl DeserializeCompact for ExecutionLedgerSubset {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        let (entry_count, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        // TODO: add entry_count checks ... see #1200
        cursor += delta;

        let mut ledger_subset = ExecutionLedgerSubset(BTreeMap::new());
        for _ in 0..entry_count {
            let address = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;

            let (data, delta) = LedgerEntry::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;

            ledger_subset.0.insert(address, data);
        }

        Ok((ledger_subset, cursor))
    }
}

impl FinalLedger {
    /// Get a part of the ledger
    /// Used for bootstrap
    /// Parameters:
    /// * cursor: Where we stopped in the ledger
    ///
    /// Returns:
    /// A subset of the ledger starting at `cursor` and of size `MAXIMUM_BYTES_MESSAGE_BOOTSTRAP` bytes.
    pub fn get_ledger_part(
        &self,
        cursor: Option<LedgerCursor>,
    ) -> Result<(Vec<u8>, LedgerCursor), ModelsError> {
        let mut next_cursor = cursor.unwrap_or(LedgerCursor(
            *self
                .sorted_ledger
                .first_key_value()
                .ok_or_else(|| ModelsError::BufferError("Ledger empty".into()))?
                .0,
            LedgerCursorStep::Start,
        ));
        let mut data = Vec::new();
        let amount_serializer = AmountSerializer::new();
        for (addr, entry) in self.sorted_ledger.range(next_cursor.0..) {
            // No match because we want to be able to pass in all if in one loop
            if let LedgerCursorStep::Finish = next_cursor.1 {
                next_cursor.1 = LedgerCursorStep::Start;
                next_cursor.0 = *addr;
            }
            if let LedgerCursorStep::Start = next_cursor.1 {
                data.extend(addr.to_bytes());
                next_cursor.1 = LedgerCursorStep::Balance;
                if data.len() as u32 > MAXIMUM_BYTES_MESSAGE_BOOTSTRAP {
                    return Ok((data, next_cursor));
                }
            }
            if let LedgerCursorStep::Balance = next_cursor.1 {
                data.extend(amount_serializer.serialize(&entry.parallel_balance)?);
                next_cursor.1 = LedgerCursorStep::StartDatastore;
                if data.len() as u32 > MAXIMUM_BYTES_MESSAGE_BOOTSTRAP {
                    return Ok((data, next_cursor));
                }
            }
            if let LedgerCursorStep::StartDatastore = next_cursor.1 {
                for (key, value) in &entry.datastore {
                    data.extend(key.to_bytes());
                    data.extend((value.len() as u64).to_varint_bytes());
                    data.extend(value);
                    next_cursor.1 = LedgerCursorStep::Datastore(*key);
                    if data.len() as u32 > MAXIMUM_BYTES_MESSAGE_BOOTSTRAP {
                        return Ok((data, next_cursor));
                    }
                }
            }
            if let LedgerCursorStep::Datastore(key) = next_cursor.1 {
                for (key, value) in entry.datastore.range((Excluded(key), Unbounded)) {
                    data.extend(key.to_bytes());
                    data.extend((value.len() as u64).to_varint_bytes());
                    data.extend(value);
                    next_cursor.1 = LedgerCursorStep::Datastore(*key);
                    if data.len() as u32 > MAXIMUM_BYTES_MESSAGE_BOOTSTRAP {
                        return Ok((data, next_cursor));
                    }
                }
            }
        }
        // Need to dereference because Prehashed trait is not implemented for &Address
        // TODO: Reimplement it
        Err(ModelsError::SerializeError("wip".into()))
    }

    /// Set a part of the ledger
    /// Used for bootstrap
    /// Parameters:
    /// * cursor: Where we stopped in the ledger
    ///
    /// Returns:
    /// Nothing on success error else.
    pub fn set_ledger_part(
        &mut self,
        old_cursor: Option<LedgerCursor>,
        new_cursor: LedgerCursor,
        data: Vec<u8>,
    ) -> Result<(), ModelsError> {
        let mut cursor_data: usize = 0;
        let mut cursor = if let Some(old_cursor) = old_cursor {
            old_cursor
        } else {
            let address =
                Address::from_bytes(&array_from_slice(&data[cursor_data..ADDRESS_SIZE_BYTES])?)?;
            cursor_data += ADDRESS_SIZE_BYTES;
            self.sorted_ledger
                .entry(address)
                .or_insert_with(LedgerEntry::default);
            LedgerCursor(address, LedgerCursorStep::Balance)
        };
        loop {
            if cursor == new_cursor {
                break;
            }
            // We want to make one check per loop to check that the cursor isn't finish each loop turn.
            match cursor.1 {
                LedgerCursorStep::Start => {
                    let address = Address::from_bytes(&array_from_slice(&data[cursor_data..])?)?;
                    self.sorted_ledger
                        .entry(address)
                        .or_insert_with(LedgerEntry::default);
                    cursor_data += ADDRESS_SIZE_BYTES;
                    cursor.1 = LedgerCursorStep::Balance;
                }
                LedgerCursorStep::Balance => {
                    let amount_deserializer = AmountDeserializer::new();
                    let (balance, delta) = amount_deserializer.deserialize(&data[cursor_data..])?;
                    self.sorted_ledger
                        .get_mut(&cursor.0)
                        .ok_or_else(|| ModelsError::InvalidLedgerChange(format!(
                            "Address: {:#?} not found",
                            cursor.0
                        )))?
                        .parallel_balance = balance;
                    cursor_data += delta;
                    cursor.1 = LedgerCursorStep::StartDatastore;
                }
                LedgerCursorStep::StartDatastore => {}
                LedgerCursorStep::Bytecode => {}
                LedgerCursorStep::Datastore(key) => {}
                LedgerCursorStep::Finish => {}
            }
        }
        Ok(())
    }
}
