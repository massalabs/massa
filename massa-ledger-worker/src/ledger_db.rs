// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Module to interact with the disk ledger

use massa_ledger_exports::*;
use massa_models::constants::{ADDRESS_SIZE_BYTES, LEDGER_PART_SIZE_MESSAGE_BYTES};
use massa_models::{
    Address, ModelsError, SerializeCompact, Slot, VecU8Deserializer, VecU8Serializer,
};
use massa_serialization::{Deserializer, Serializer};
use nom::multi::many0;
use nom::sequence::tuple;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Bound;
use std::path::PathBuf;
use std::rc::Rc;

/// Ledger sub entry enum
pub enum LedgerSubEntry {
    /// Balance
    Balance,
    /// Bytecode
    Bytecode,
    /// Datastore entry
    Datastore(Vec<u8>),
}

/// Disk ledger DB module
///
/// Contains a RocksDB DB instance
#[derive(Debug)]
pub(crate) struct LedgerDB(BTreeMap<Vec<u8>, Vec<u8>>);

/// For a given start prefix (inclusive), returns the correct end prefix (non-inclusive).
/// This assumes the key bytes are ordered in lexicographical order.
/// Since key length is not limited, for some case we return `None` because there is
/// no bounded limit (every keys in the serie `[]`, `[255]`, `[255, 255]` ...).
fn end_prefix(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end_range = prefix.to_vec();
    while let Some(0xff) = end_range.last() {
        end_range.pop();
    }
    if let Some(byte) = end_range.last_mut() {
        *byte += 1;
        Some(end_range)
    } else {
        None
    }
}

#[test]
fn test_end_prefix() {
    assert_eq!(end_prefix(&[5, 6, 7]), Some(vec![5, 6, 8]));
    assert_eq!(end_prefix(&[5, 6, 255]), Some(vec![5, 7]));
}

// TODO: save attached slot in metadata for a lighter bootstrap after disconnection
impl LedgerDB {
    /// Create and initialize a new LedgerDB.
    ///
    /// # Arguments
    /// * path: path to the desired disk ledger db directory
    pub fn new(_path: PathBuf) -> Self {
        LedgerDB(BTreeMap::default())
    }

    /// Set the initial disk ledger
    ///
    /// # Arguments
    /// * initial_ledger: initial entries to put in the disk
    pub fn set_initial_ledger(&mut self, initial_ledger: HashMap<Address, LedgerEntry>) {
        for (address, entry) in initial_ledger {
            self.put_entry(&address, entry);
        }
    }

    /// Allows applying `LedgerChanges` to the disk ledger
    ///
    /// # Arguments
    /// * changes: ledger changes to be applied
    /// * slot: new slot associated to the final ledger
    pub fn apply_changes(&mut self, changes: LedgerChanges, _slot: Slot) {
        // for all incoming changes
        for (addr, change) in changes.0 {
            match change {
                // the incoming change sets a ledger entry to a new one
                SetUpdateOrDelete::Set(new_entry) => {
                    // inserts/overwrites the entry with the incoming one
                    self.put_entry(&addr, new_entry);
                }
                // the incoming change updates an existing ledger entry
                SetUpdateOrDelete::Update(entry_update) => {
                    // applies the updates to the entry
                    // if the entry does not exist, inserts a default one and applies the updates to it
                    self.update_entry(&addr, entry_update);
                }
                // the incoming change deletes a ledger entry
                SetUpdateOrDelete::Delete => {
                    // delete the entry, if it exists
                    self.delete_entry(&addr);
                }
            }
        }
    }

    /// Add every sub-entry individually for a given entry.
    ///
    /// # Arguments
    /// * addr: associated address
    /// * ledger_entry: complete entry to be added
    /// * batch: the given operation batch to update
    fn put_entry(&mut self, addr: &Address, ledger_entry: LedgerEntry) {
        // balance
        // Amount::to_bytes_compact() never fails
        self.0.insert(
            balance_key!(addr),
            ledger_entry.parallel_balance.to_bytes_compact().unwrap(),
        );

        // bytecode
        self.0.insert(bytecode_key!(addr), ledger_entry.bytecode);

        // datastore
        for (hash, entry) in ledger_entry.datastore {
            self.0.insert(data_key!(addr, hash), entry);
        }
    }

    /// Get the given sub-entry of a given address.
    ///
    /// # Arguments
    /// * addr: associated address
    /// * ty: type of the queried sub-entry
    ///
    /// # Returns
    /// An Option of the sub-entry value as bytes
    pub fn get_sub_entry(&self, addr: &Address, ty: LedgerSubEntry) -> Option<Vec<u8>> {
        match ty {
            LedgerSubEntry::Balance => self.0.get(&balance_key!(addr)).cloned(),
            LedgerSubEntry::Bytecode => self.0.get(&bytecode_key!(addr)).cloned(),
            LedgerSubEntry::Datastore(hash) => self.0.get(&data_key!(addr, hash)).cloned(),
        }
    }

    /// Get every key of the datastore for a given address.
    ///
    /// # Returns
    /// A BTreeSet of the datastore keys
    pub fn get_datastore_keys(&self, _addr: &Address) -> BTreeSet<Vec<u8>> {
        self.0
            .iter()
            .map(|(key, _)| key.split_at(ADDRESS_SIZE_BYTES + 1).1.to_vec())
            .collect()
    }

    /// Update the ledger entry of a given address.
    ///
    /// # Arguments
    /// * entry_update: a descriptor of the entry updates to be applied
    /// * batch: the given operation batch to update
    fn update_entry(&mut self, addr: &Address, entry_update: LedgerEntryUpdate) {
        // balance
        if let SetOrKeep::Set(balance) = entry_update.parallel_balance {
            self.0
                .insert(balance_key!(addr), balance.to_bytes_compact().unwrap());
        }

        // bytecode
        if let SetOrKeep::Set(bytecode) = entry_update.bytecode {
            self.0.insert(bytecode_key!(addr), bytecode);
        }

        // datastore
        for (hash, update) in entry_update.datastore {
            match update {
                SetOrDelete::Set(entry) => self.0.insert(data_key!(addr, hash), entry),
                SetOrDelete::Delete => self.0.remove(&data_key!(addr, hash)),
            };
        }
    }

    /// Delete every sub-entry associated to the given address.
    ///
    /// # Arguments
    /// * batch: the given operation batch to update
    fn delete_entry(&mut self, addr: &Address) {
        // balance
        self.0.remove(&balance_key!(addr));

        // bytecode
        self.0.remove(&bytecode_key!(addr));

        // datastore
        let lower = data_prefix!(addr).clone();
        let upper = end_prefix(data_prefix!(addr)).unwrap();
        let mut keys = Vec::default();
        for (key, _) in self.0.range(lower..upper) {
            keys.push(key.clone());
        }
        for fmt_key in keys {
            self.0.remove(&fmt_key);
        }
    }

    /// Get a part of the disk Ledger.
    /// Mainly used in the bootstrap process.
    ///
    /// # Arguments
    /// * last_key: key where the part retrieving must start
    ///
    /// # Returns
    /// A tuple containing:
    /// * The ledger part as bytes
    /// * The last taken key (this is an optimization to easily keep a reference to the last key)
    pub fn get_ledger_part(
        &self,
        last_key: &Option<Vec<u8>>,
    ) -> Result<(Vec<u8>, Option<Vec<u8>>), ModelsError> {
        let ser = VecU8Serializer::new();
        let key_serializer = KeySerializer::new();
        let mut part = Vec::new();
        let mut last_taken_key = None;

        // Iterates over the whole database
        let mut iter = self.0.range(last_key.clone().unwrap_or_default()..);
        if last_key.is_some() {
            iter.next();
        }
        for (key, entry) in iter {
            if (part.len() as u64) < (LEDGER_PART_SIZE_MESSAGE_BYTES) {
                key_serializer.serialize(&key.to_vec(), &mut part)?;
                ser.serialize(&entry.to_vec(), &mut part)?;
                last_taken_key = Some(key.to_vec());
            } else {
                break;
            }
        }
        Ok((part, last_taken_key))
    }

    /// Set a part of the ledger in the database.
    /// We deserialize in this function because we insert in the ledger while deserializing.
    /// Used for bootstrap.
    ///
    /// # Arguments
    /// * data: must be the serialized version provided by `get_ledger_part`
    ///
    /// # Returns
    /// The last key of the inserted entry (this is an optimization to easily keep a reference to the last key)
    pub fn set_ledger_part<'a>(&mut self, data: &'a [u8]) -> Result<Option<Vec<u8>>, ModelsError> {
        let vec_u8_deserializer =
            VecU8Deserializer::new(Bound::Included(0), Bound::Excluded(u64::MAX));
        let key_deserializer = KeyDeserializer::new();
        let mut last_key = Rc::new(None);

        // Since this data is coming from the network, deser to address and ser back to bytes for a security check.
        let (rest, _) = many0(|input: &'a [u8]| {
            let (rest, (key, value)) = tuple((
                |input| key_deserializer.deserialize(input),
                |input| vec_u8_deserializer.deserialize(input),
            ))(input)?;
            *Rc::get_mut(&mut last_key).ok_or_else(|| {
                nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Fail))
            })? = Some(key.clone());
            self.0.insert(key, value);
            Ok((rest, ()))
        })(data)
        .map_err(|_| ModelsError::SerializeError("Error in deserialization".to_string()))?;

        // Every byte should have been read
        if rest.is_empty() {
            Ok((*last_key).clone())
        } else {
            println!("REST LEN = {}", rest.len());
            Err(ModelsError::SerializeError(
                "rest is not empty.".to_string(),
            ))
        }
    }

    /// Get every address and their corresponding balance.
    ///
    /// IMPORTANT: This should only be used for debug purposes.
    ///
    /// # Returns
    /// A BTreeMap with the address as key and the balance as value
    #[cfg(feature = "testing")]
    pub fn get_every_address(&self) -> std::collections::BTreeMap<Address, massa_models::Amount> {
        use massa_models::{address::AddressDeserializer, DeserializeCompact};
        use massa_serialization::DeserializeError;

        let mut addresses = std::collections::BTreeMap::new();
        let address_deserializer = AddressDeserializer::new();
        for (key, entry) in self.0.iter() {
            let (rest, address) = address_deserializer
                .deserialize::<DeserializeError>(&key[..])
                .unwrap();
            if rest.first() == Some(&BALANCE_IDENT) {
                addresses.insert(
                    address,
                    massa_models::Amount::from_bytes_compact(&entry).unwrap().0,
                );
            }
        }
        addresses
    }

    /// Get the entire datastore for a given address.
    ///
    /// IMPORTANT: This should only be used for debug purposes.
    ///
    /// # Returns
    /// A BTreeMap with the entry hash as key and the data bytes as value
    #[cfg(feature = "testing")]
    pub fn get_entire_datastore(
        &self,
        addr: &Address,
    ) -> std::collections::BTreeMap<Vec<u8>, Vec<u8>> {
        let lower = data_prefix!(addr).clone();
        let upper = end_prefix(data_prefix!(addr)).unwrap();
        self.0
            .range(lower..upper)
            .map(|(key, data)| {
                (
                    key.split_at(ADDRESS_SIZE_BYTES + 1).1.to_vec(),
                    data.to_vec(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::LedgerDB;
    use crate::ledger_db::LedgerSubEntry;
    use massa_ledger_exports::{LedgerEntry, LedgerEntryUpdate, SetOrKeep};
    use massa_models::{Address, Amount, DeserializeCompact};
    use massa_signature::KeyPair;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    #[cfg(test)]
    fn init_test_ledger(addr: Address) -> (LedgerDB, BTreeMap<Vec<u8>, Vec<u8>>) {
        // init data
        let mut data = BTreeMap::new();
        data.insert(b"1".to_vec(), b"a".to_vec());
        data.insert(b"2".to_vec(), b"b".to_vec());
        data.insert(b"3".to_vec(), b"c".to_vec());
        let entry = LedgerEntry {
            parallel_balance: Amount::from_raw(42),
            datastore: data.clone(),
            ..Default::default()
        };
        let entry_update = LedgerEntryUpdate {
            parallel_balance: SetOrKeep::Set(Amount::from_raw(21)),
            bytecode: SetOrKeep::Keep,
            ..Default::default()
        };

        // write data
        let temp_dir = TempDir::new().unwrap();
        let mut db = LedgerDB::new(temp_dir.path().to_path_buf());
        db.put_entry(&addr, entry);
        db.update_entry(&addr, entry_update);

        // return db and initial data
        (db, data)
    }

    /// Functional test of LedgerDB
    #[test]
    fn test_ledger_db() {
        // init addresses
        let pub_a = KeyPair::generate().get_public_key();
        let pub_b = KeyPair::generate().get_public_key();
        let a = Address::from_public_key(&pub_a);
        let b = Address::from_public_key(&pub_b);
        let (mut db, data) = init_test_ledger(a);

        // first assert
        assert!(db.get_sub_entry(&a, LedgerSubEntry::Balance).is_some());
        assert_eq!(
            Amount::from_bytes_compact(&db.get_sub_entry(&a, LedgerSubEntry::Balance).unwrap())
                .unwrap()
                .0,
            Amount::from_raw(21)
        );
        assert!(db.get_sub_entry(&b, LedgerSubEntry::Balance).is_none());
        assert_eq!(data, db.get_entire_datastore(&a));

        // delete entry
        db.delete_entry(&a);

        // second assert
        assert!(db.get_sub_entry(&a, LedgerSubEntry::Balance).is_none());
        assert!(db.get_entire_datastore(&a).is_empty());
    }

    #[test]
    fn test_ledger_parts() {
        let pub_a = KeyPair::generate().get_public_key();
        let a = Address::from_public_key(&pub_a);
        let (mut db, _) = init_test_ledger(a);
        let res = db.get_ledger_part(&None).unwrap();
        db.set_ledger_part(&res.0[..]).unwrap();
    }
}
