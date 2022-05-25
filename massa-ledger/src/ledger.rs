// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final ledger associating addresses to their balances, bytecode and data.

use crate::ledger_changes::LedgerChanges;
use crate::ledger_db::{LedgerDB, LedgerSubEntry};
use crate::ledger_entry::LedgerEntry;
use crate::types::{Applicable, SetUpdateOrDelete};
use crate::{FinalLedgerBootstrapState, LedgerConfig, LedgerError};
use massa_hash::Hash;
use massa_models::{Address, Amount, DeserializeCompact};
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
                    self.sorted_ledger.put_entry(&addr, new_entry);
                }
                // the incoming change updates an existing ledger entry
                SetUpdateOrDelete::Update(entry_update) => {
                    // applies the updates to the entry
                    // if the entry does not exist, inserts a default one and applies the updates to it
                    self.sorted_ledger.update_entry(&addr, entry_update);
                }
                // the incoming change deletes a ledger entry
                SetUpdateOrDelete::Delete => {
                    // delete the entry, if it exists
                    self.sorted_ledger.delete_entry(&addr);
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
        let mut sorted_ledger = LedgerDB::new();

        // load the ledger tree from file
        let initial_ledger = serde_json::from_str::<BTreeMap<Address, Amount>>(
            &std::fs::read_to_string(&config.initial_sce_ledger_path)
                .map_err(init_file_error!("loading", config))?,
        )
        .map_err(init_file_error!("parsing", config))?;

        // put_entry initial ledger values in the disk db
        for (address, amount) in &initial_ledger {
            sorted_ledger.put_entry(
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
    pub fn from_bootstrap_state(_config: LedgerConfig, state: FinalLedgerBootstrapState) -> Self {
        // temporary implementation while waiting for streaming
        let mut db = LedgerDB::new();
        for (key, entry) in state.sorted_ledger {
            db.put_entry(&key, entry);
        }
        FinalLedger {
            sorted_ledger: db,
            _config,
        }
    }

    /// Gets a snapshot of the ledger to bootstrap other nodes
    ///
    /// TODO: This loads the whole ledger in RAM. Switch to streaming in the future
    pub fn get_bootstrap_state(&self) -> FinalLedgerBootstrapState {
        // temporary implementation while waiting for streaming
        FinalLedgerBootstrapState {
            sorted_ledger: self
                .sorted_ledger
                .get_every_address()
                .iter()
                .map(|(addr, balance)| {
                    (
                        *addr,
                        LedgerEntry {
                            parallel_balance: *balance,
                            bytecode: self
                                .sorted_ledger
                                .get_sub_entry(addr, LedgerSubEntry::Bytecode)
                                .unwrap_or_default(),
                            datastore: self.sorted_ledger.get_entire_datastore(addr),
                        },
                    )
                })
                .collect(),
        }
    }

    /// Gets the parallel balance of a ledger entry
    ///
    /// # Returns
    /// The parallel balance, or None if the ledger entry was not found
    pub fn get_parallel_balance(&self, addr: &Address) -> Option<Amount> {
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
    pub fn get_bytecode(&self, addr: &Address) -> Option<Vec<u8>> {
        self.sorted_ledger
            .get_sub_entry(addr, LedgerSubEntry::Bytecode)
    }

    /// Checks if a ledger entry exists
    ///
    /// # Returns
    /// true if it exists, false otherwise.
    pub fn entry_exists(&self, addr: &Address) -> bool {
        // note: document the "may"
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
    pub fn get_data_entry(&self, addr: &Address, key: &Hash) -> Option<Vec<u8>> {
        self.sorted_ledger
            .get_sub_entry(addr, LedgerSubEntry::Datastore(*key))
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
            .get_sub_entry(addr, LedgerSubEntry::Datastore(*key))
            .is_some()
    }

    /// Gets the entire datastore of a given address.
    ///
    /// # Returns
    /// A copy of the datastore sorted by key
    pub fn get_entire_datastore(&self, addr: &Address) -> BTreeMap<Hash, Vec<u8>> {
        self.sorted_ledger.get_entire_datastore(addr)
    }

    // /// Get a part of the ledger
    // /// Used for bootstrap
    // /// Parameters:
    // /// * cursor: Where we stopped in the ledger
    // ///
    // /// Returns:
    // /// A subset of the ledger starting at `cursor` and of size `LEDGER_PART_SIZE_MESSAGE_BYTES` bytes.
    // pub fn get_ledger_part(
    //     &self,
    //     cursor: Option<LedgerCursor>,
    // ) -> Result<(Vec<u8>, Option<LedgerCursor>), ModelsError> {
    //     let mut next_cursor = if let Some(cursor) = cursor.or_else(|| {
    //         self.sorted_ledger
    //             .first_key_value()
    //             .map(|(&address, _)| LedgerCursor {
    //                 address,
    //                 step: LedgerCursorStep::Start,
    //             })
    //     }) {
    //         cursor
    //     } else {
    //         return Ok((vec![], None));
    //     };
    //     let mut data = Vec::new();
    //     let amount_serializer = AmountSerializer::new(Included(u64::MIN), Included(u64::MAX));
    //     for (addr, entry) in self.sorted_ledger.range(next_cursor.address..) {
    //         while (data.len() as u64) < LEDGER_PART_SIZE_MESSAGE_BYTES {
    //             match next_cursor.step {
    //                 LedgerCursorStep::Start => {
    //                     data.extend(addr.to_bytes());
    //                     next_cursor.step = LedgerCursorStep::Balance;
    //                 }
    //                 LedgerCursorStep::Balance => {
    //                     data.extend(amount_serializer.serialize(&entry.parallel_balance)?);
    //                     next_cursor.step = LedgerCursorStep::Bytecode;
    //                 }
    //                 LedgerCursorStep::Bytecode => {
    //                     data.extend((entry.bytecode.len() as u64).to_varint_bytes());
    //                     data.extend(&entry.bytecode);
    //                     next_cursor.step = LedgerCursorStep::Datastore(None);
    //                 }
    //                 LedgerCursorStep::Datastore(key) => {
    //                     let key = if let Some(key) = key {
    //                         key
    //                     } else if let Some((&key, _)) = entry.datastore.first_key_value() {
    //                         key
    //                     } else {
    //                         next_cursor.step = LedgerCursorStep::Finish;
    //                         break;
    //                     };
    //                     for (key, value) in entry.datastore.range(key..) {
    //                         data.push(DATASTORE_KEY_IDENTIFIER);
    //                         data.extend(key.to_bytes());
    //                         data.extend((value.len() as u64).to_varint_bytes());
    //                         data.extend(value);
    //                         next_cursor.step = LedgerCursorStep::Datastore(Some(*key));
    //                         if data.len() as u64 > LEDGER_PART_SIZE_MESSAGE_BYTES {
    //                             return Ok((data, Some(next_cursor)));
    //                         }
    //                     }
    //                     next_cursor.step = LedgerCursorStep::Finish;
    //                 }
    //                 LedgerCursorStep::Finish => {
    //                     data.push(DATASTORE_END_IDENTIFIER);
    //                     next_cursor.step = LedgerCursorStep::Start;
    //                     next_cursor.address = *addr;
    //                     break;
    //                 }
    //             }
    //             if data.len() as u64 > LEDGER_PART_SIZE_MESSAGE_BYTES {
    //                 return Ok((data, Some(next_cursor)));
    //             }
    //         }
    //     }
    //     Ok((data, Some(next_cursor)))
    // }

    // /// Set a part of the ledger
    // /// Used for bootstrap
    // /// Parameters:
    // /// * cursor: Where we stopped in the ledger
    // ///
    // /// Returns:
    // /// Nothing on success error else.
    // pub fn set_ledger_part(
    //     &mut self,
    //     old_cursor: Option<LedgerCursor>,
    //     data: Vec<u8>,
    // ) -> Result<Option<LedgerCursor>, ModelsError> {
    //     let mut data = data.as_bytes();
    //     let address_deserializer = AddressDeserializer::new();
    //     let hash_deserializer = HashDeserializer::default();
    //     let amount_deserializer = AmountDeserializer::new(Included(u64::MIN), Included(u64::MAX));
    //     let vecu8_deserializer = VecU8Deserializer::new(Included(u64::MIN), Included(u64::MAX));
    //     let mut cursor = if let Some(old_cursor) = old_cursor {
    //         old_cursor
    //     } else {
    //         if data.is_empty() {
    //             return Ok(None);
    //         }
    //         let (rest, address) = address_deserializer.deserialize(data).map_err(|_| {
    //             ModelsError::DeserializeError("Fail to deserialize address".to_string())
    //         })?;
    //         data = rest;
    //         self.sorted_ledger
    //             .entry(address)
    //             .or_insert_with(LedgerEntry::default);
    //         LedgerCursor {
    //             address,
    //             step: LedgerCursorStep::Balance,
    //         }
    //     };
    //     while !data.is_empty() {
    //         // We want to make one check per loop to check that the cursor isn't finish each loop turn.
    //         let (new_state, rest) = match cursor.step {
    //             LedgerCursorStep::Start => {
    //                 let (rest, address) = address_deserializer.deserialize(data).map_err(|_| {
    //                     ModelsError::DeserializeError("Fail to deserialize address".to_string())
    //                 })?;
    //                 self.sorted_ledger
    //                     .entry(address)
    //                     .or_insert_with(LedgerEntry::default);
    //                 cursor.address = address;
    //                 (LedgerCursorStep::Balance, rest)
    //             }
    //             LedgerCursorStep::Balance => {
    //                 let (rest, balance) = amount_deserializer.deserialize(data).map_err(|_| {
    //                     ModelsError::DeserializeError("Fail to deserialize amount".to_string())
    //                 })?;
    //                 self.sorted_ledger
    //                     .get_mut(&cursor.address)
    //                     .ok_or_else(|| {
    //                         ModelsError::InvalidLedgerChange(format!(
    //                             "Address: {:#?} not found",
    //                             cursor.address
    //                         ))
    //                     })?
    //                     .parallel_balance = balance;
    //                 (LedgerCursorStep::Bytecode, rest)
    //             }
    //             LedgerCursorStep::Bytecode => {
    //                 let (rest, bytecode) = vecu8_deserializer.deserialize(data).map_err(|_| {
    //                     ModelsError::DeserializeError("Fail to deserialize bytecode".to_string())
    //                 })?;
    //                 self.sorted_ledger
    //                     .get_mut(&cursor.address)
    //                     .ok_or_else(|| {
    //                         ModelsError::InvalidLedgerChange(format!(
    //                             "Address: {:#?} not found",
    //                             cursor.address
    //                         ))
    //                     })?
    //                     .bytecode = bytecode;
    //                 (LedgerCursorStep::Datastore(None), rest)
    //             }
    //             LedgerCursorStep::Datastore(_) => {
    //                 match data.get(0) {
    //                     Some(&DATASTORE_END_IDENTIFIER) => {
    //                         cursor.step = LedgerCursorStep::Finish;
    //                         continue;
    //                     }
    //                     Some(_) => (),
    //                     None => {
    //                         return Err(ModelsError::DeserializeError(
    //                             "No identifier for datastore key when excepted".to_string(),
    //                         ))
    //                     }
    //                 };
    //                 data = match data.get(1..) {
    //                     Some(data) => data,
    //                     None => {
    //                         return Err(ModelsError::DeserializeError(
    //                             "No datastore key when excepted".to_string(),
    //                         ))
    //                     }
    //                 };
    //                 let mut entry_parser = tuple((
    //                     context("Key of datastore deserialization", |input| {
    //                         hash_deserializer.deserialize(input)
    //                     }),
    //                     context("Value of a key of datastore deserialization", |input| {
    //                         vecu8_deserializer.deserialize(input)
    //                     }),
    //                 ));
    //                 let (rest, (key, value)) = entry_parser(data)
    //                     .map_err(|err| ModelsError::DeserializeError(err.to_string()))?;
    //                 self.sorted_ledger
    //                     .get_mut(&cursor.address)
    //                     .ok_or_else(|| {
    //                         ModelsError::InvalidLedgerChange(format!(
    //                             "Address: {:#?} not found",
    //                             cursor.address
    //                         ))
    //                     })?
    //                     .datastore
    //                     .insert(key, value);
    //                 (LedgerCursorStep::Datastore(Some(key)), rest)
    //             }
    //             LedgerCursorStep::Finish => (
    //                 LedgerCursorStep::Start,
    //                 data.get(1..).ok_or_else(|| {
    //                     ModelsError::DeserializeError("Missing end of message".to_string())
    //                 })?,
    //             ),
    //         };
    //         cursor.step = new_state;
    //         data = rest;
    //     }
    //     Ok(Some(cursor))
    // }
}

// #[cfg(test)]
// mod tests {
//     use std::collections::BTreeMap;

//     use crate::{FinalLedger, LedgerConfig, LedgerEntry};
//     use massa_hash::Hash;
//     use massa_models::{Address, Amount};

//     #[test]
//     fn test_part_ledger() {
//         let mut ledger: FinalLedger =
//             FinalLedger::new(LedgerConfig::sample(&BTreeMap::new()).0).unwrap();
//         ledger.sorted_ledger.clear();
//         let mut datastore = BTreeMap::new();
//         datastore.insert(Hash::compute_from(&"hello".as_bytes()), vec![4, 5, 6]);
//         datastore.insert(Hash::compute_from(&"world".as_bytes()), vec![4, 5, 6]);
//         let ledger_entry = LedgerEntry {
//             parallel_balance: Amount::from_raw(10),
//             bytecode: vec![1, 2, 3],
//             datastore,
//         };
//         ledger.sorted_ledger.insert(
//             Address::from_bs58_check("xh1fXpp7VuciaCwejMF7ufF19SWv7dFPJ7U6HiTQaeNEFBiV3").unwrap(),
//             ledger_entry,
//         );
//         let (part, cursor) = ledger.get_ledger_part(None).unwrap();
//         let (part2, cursor2) = ledger.get_ledger_part(cursor.clone()).unwrap();
//         let (part3, _) = ledger.get_ledger_part(cursor2.clone()).unwrap();
//         let mut new_ledger: FinalLedger = FinalLedger::new(LedgerConfig {
//             initial_sce_ledger_path: "../massa-node/base_config/initial_sce_ledger.json".into(),
//         })
//         .unwrap();
//         new_ledger.sorted_ledger.clear();
//         let cursor = new_ledger.set_ledger_part(None, part).unwrap();
//         let cursor = new_ledger.set_ledger_part(cursor, part2).unwrap();
//         new_ledger.set_ledger_part(cursor, part3).unwrap();
//         assert_eq!(ledger.sorted_ledger, new_ledger.sorted_ledger);
//     }

//     #[test]
//     fn test_part_ledger_empty() {
//         let mut ledger: FinalLedger =
//             FinalLedger::new(LedgerConfig::sample(&BTreeMap::new()).0).unwrap();
//         ledger.sorted_ledger.clear();
//         let (part, old_cursor) = ledger.get_ledger_part(None).unwrap();
//         assert!(old_cursor.is_none());
//         let mut new_ledger: FinalLedger = FinalLedger::new(LedgerConfig {
//             initial_sce_ledger_path: "../massa-node/base_config/initial_sce_ledger.json".into(),
//         })
//         .unwrap();
//         new_ledger.sorted_ledger.clear();
//         let cursor = new_ledger.set_ledger_part(None, part).unwrap();
//         assert!(cursor.is_none());
//         assert_eq!(old_cursor, cursor);
//         assert_eq!(ledger.sorted_ledger, new_ledger.sorted_ledger);
//     }
// }
