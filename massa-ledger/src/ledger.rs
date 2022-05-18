// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final ledger associating addresses to their balances, bytecode and data.

use crate::cursor::{LedgerCursor, LedgerCursorStep};
use crate::ledger_changes::LedgerChanges;
use crate::ledger_entry::LedgerEntry;
use crate::types::{Applicable, SetUpdateOrDelete};
use crate::{FinalLedgerBootstrapState, LedgerConfig, LedgerError};
use massa_hash::{Hash, HashDeserializer};
use massa_models::address::AddressDeserializer;
use massa_models::amount::{AmountDeserializer, AmountSerializer};
use massa_models::constants::LEDGER_PART_SIZE_MESSAGE_BYTES;
use massa_models::{
    Address, Amount, ModelsError, SerializeVarInt, U64VarIntDeserializer, VecU8Deserializer,
};
use massa_serialization::{Deserializer, Serializer};
use nom::AsBytes;
use std::collections::BTreeMap;
use std::ops::Bound;

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

const DATASTORE_END_IDENTIFIER: u8 = 0;
const DATASTORE_KEY_IDENTIFIER: u8 = 1;

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

    /// Get a part of the ledger
    /// Used for bootstrap
    /// Parameters:
    /// * cursor: Where we stopped in the ledger
    ///
    /// Returns:
    /// A subset of the ledger starting at `cursor` and of size `LEDGER_PART_SIZE_MESSAGE_BYTES` bytes.
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
            while (data.len() as u64) < LEDGER_PART_SIZE_MESSAGE_BYTES {
                match next_cursor.1 {
                    LedgerCursorStep::Start => {
                        data.extend(addr.to_bytes());
                        next_cursor.1 = LedgerCursorStep::Balance;
                    }
                    LedgerCursorStep::Balance => {
                        data.extend(amount_serializer.serialize(&entry.parallel_balance)?);
                        next_cursor.1 = LedgerCursorStep::Bytecode;
                    }
                    LedgerCursorStep::Bytecode => {
                        data.extend((entry.bytecode.len() as u64).to_varint_bytes());
                        data.extend(&entry.bytecode);
                        next_cursor.1 = LedgerCursorStep::Datastore(None);
                    }
                    LedgerCursorStep::Datastore(key) => {
                        let key = if let Some(key) = key {
                            key
                        } else if let Some((&key, _)) = entry.datastore.first_key_value() {
                            key
                        } else {
                            next_cursor.1 = LedgerCursorStep::Finish;
                            break;
                        };
                        for (key, value) in entry.datastore.range(key..) {
                            data.push(DATASTORE_KEY_IDENTIFIER);
                            data.extend(key.to_bytes());
                            data.extend((value.len() as u64).to_varint_bytes());
                            data.extend(value);
                            next_cursor.1 = LedgerCursorStep::Datastore(Some(*key));
                            if data.len() as u64 > LEDGER_PART_SIZE_MESSAGE_BYTES {
                                return Ok((data, next_cursor));
                            }
                        }
                        next_cursor.1 = LedgerCursorStep::Finish;
                    }
                    LedgerCursorStep::Finish => {
                        data.push(DATASTORE_END_IDENTIFIER);
                        next_cursor.1 = LedgerCursorStep::Start;
                        next_cursor.0 = *addr;
                        break;
                    }
                }
                if data.len() as u64 > LEDGER_PART_SIZE_MESSAGE_BYTES {
                    return Ok((data, next_cursor));
                }
            }
        }
        Ok((data, next_cursor))
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
        let mut data = data.as_bytes();
        let address_deserializer = AddressDeserializer::new();
        let hash_deserializer = HashDeserializer::new();
        let mut cursor = if let Some(old_cursor) = old_cursor {
            old_cursor
        } else {
            let (rest, address) = address_deserializer.deserialize(data).map_err(|_| {
                ModelsError::DeserializeError("Fail to deserialize address".to_string())
            })?;
            data = rest;
            self.sorted_ledger
                .entry(address)
                .or_insert_with(LedgerEntry::default);
            LedgerCursor(address, LedgerCursorStep::Balance)
        };
        let u64_deserializer =
            U64VarIntDeserializer::new(Bound::Included(0), Bound::Included(u64::MAX));
        let vecu8_deserializer = VecU8Deserializer::new();
        while cursor != new_cursor {
            // We want to make one check per loop to check that the cursor isn't finish each loop turn.
            match cursor.1 {
                LedgerCursorStep::Start => {
                    let (rest, address) = address_deserializer.deserialize(data).map_err(|_| {
                        ModelsError::DeserializeError("Fail to deserialize address".to_string())
                    })?;
                    data = rest;
                    self.sorted_ledger
                        .entry(address)
                        .or_insert_with(LedgerEntry::default);
                    cursor.1 = LedgerCursorStep::Balance;
                }
                LedgerCursorStep::Balance => {
                    let amount_deserializer = AmountDeserializer::new();
                    let (rest, balance) = amount_deserializer.deserialize(data).map_err(|_| {
                        ModelsError::DeserializeError("Fail to deserialize amount".to_string())
                    })?;
                    data = rest;
                    self.sorted_ledger
                        .get_mut(&cursor.0)
                        .ok_or_else(|| {
                            ModelsError::InvalidLedgerChange(format!(
                                "Address: {:#?} not found",
                                cursor.0
                            ))
                        })?
                        .parallel_balance = balance;
                    cursor.1 = LedgerCursorStep::Bytecode;
                }
                LedgerCursorStep::Bytecode => {
                    let (rest, bytecode_len) =
                        u64_deserializer.deserialize(data).map_err(|_| {
                            ModelsError::DeserializeError(
                                "Fail to deserialize len bytecode".to_string(),
                            )
                        })?;
                    data = rest;
                    let bytecode = data[..bytecode_len as usize].to_vec();
                    self.sorted_ledger
                        .get_mut(&cursor.0)
                        .ok_or_else(|| {
                            ModelsError::InvalidLedgerChange(format!(
                                "Address: {:#?} not found",
                                cursor.0
                            ))
                        })?
                        .bytecode = bytecode;
                    data = &data[bytecode_len as usize..];
                    cursor.1 = LedgerCursorStep::Datastore(None);
                }
                LedgerCursorStep::Datastore(_) => {
                    if data[0] == DATASTORE_END_IDENTIFIER {
                        cursor.1 = LedgerCursorStep::Finish;
                        continue;
                    }
                    data = &data[1..];
                    let (rest, key) = hash_deserializer.deserialize(data).map_err(|_| {
                        ModelsError::DeserializeError(
                            "Fail to deserialize key datastore".to_string(),
                        )
                    })?;
                    data = rest;
                    let (rest, value) = vecu8_deserializer.deserialize(data).map_err(|_| {
                        ModelsError::DeserializeError(format!(
                            "Fail to deserialize value of address {:#?}",
                            key
                        ))
                    })?;
                    data = rest;
                    self.sorted_ledger
                        .get_mut(&cursor.0)
                        .ok_or_else(|| {
                            ModelsError::InvalidLedgerChange(format!(
                                "Address: {:#?} not found",
                                cursor.0
                            ))
                        })?
                        .datastore
                        .insert(key, value);
                    cursor.1 = LedgerCursorStep::Datastore(Some(key));
                }
                LedgerCursorStep::Finish => {
                    cursor.1 = LedgerCursorStep::Start;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{FinalLedger, LedgerConfig, LedgerEntry};
    use massa_hash::Hash;
    use massa_models::{Address, Amount};

    #[test]
    fn test_part_ledger() {
        let mut ledger: FinalLedger =
            FinalLedger::new(LedgerConfig::sample(&BTreeMap::new()).0).unwrap();
        let mut datastore = BTreeMap::new();
        datastore.insert(Hash::compute_from(&"hello".as_bytes()), vec![4, 5, 6]);
        datastore.insert(Hash::compute_from(&"world".as_bytes()), vec![4, 5, 6]);
        let ledger_entry = LedgerEntry {
            parallel_balance: Amount::from_raw(10),
            bytecode: vec![1, 2, 3],
            datastore,
        };
        ledger.sorted_ledger.insert(
            Address::from_bs58_check("xh1fXpp7VuciaCwejMF7ufF19SWv7dFPJ7U6HiTQaeNEFBiV3").unwrap(),
            ledger_entry,
        );
        let (part, cursor) = ledger.get_ledger_part(None).unwrap();
        let (part2, cursor2) = ledger.get_ledger_part(Some(cursor.clone())).unwrap();
        let (part3, cursor3) = ledger.get_ledger_part(Some(cursor2.clone())).unwrap();
        let mut new_ledger: FinalLedger = FinalLedger::new(LedgerConfig {
            initial_sce_ledger_path: "../massa-node/base_config/initial_sce_ledger.json".into(),
        })
        .unwrap();
        new_ledger
            .set_ledger_part(None, cursor.clone(), part)
            .unwrap();
        new_ledger
            .set_ledger_part(Some(cursor), cursor2.clone(), part2)
            .unwrap();
        new_ledger
            .set_ledger_part(Some(cursor2), cursor3.clone(), part3)
            .unwrap();
        assert_eq!(ledger.sorted_ledger, new_ledger.sorted_ledger);
    }
}
