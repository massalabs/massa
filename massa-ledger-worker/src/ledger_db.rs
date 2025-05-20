//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Module to interact with the disk ledger

use massa_db_exports::{
    DBBatch, MassaDBController, MassaDirection, MassaIteratorMode, ShareableMassaDBController,
    CRUD_ERROR, KEY_SER_ERROR, LEDGER_PREFIX, STATE_CF,
};
use massa_ledger_exports::*;
use massa_models::amount::AmountDeserializer;
use massa_models::bytecode::BytecodeDeserializer;
use massa_models::datastore::{get_prefix_bounds, range_intersection};
use massa_models::types::{SetOrDelete, SetOrKeep, SetUpdateOrDelete};
use massa_models::{
    address::Address, amount::AmountSerializer, bytecode::BytecodeSerializer, slot::Slot,
};
use massa_serialization::{
    DeserializeError, Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use parking_lot::{lock_api::RwLockReadGuard, RawRwLock};
use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;

use massa_models::amount::Amount;
use std::ops::Bound;

/// Ledger sub entry enum
pub enum LedgerSubEntry {
    /// Version
    Version,
    /// Balance
    Balance,
    /// Bytecode
    Bytecode,
    /// Datastore entry
    Datastore(Vec<u8>),
}

impl LedgerSubEntry {
    fn derive_key(&self, addr: &Address) -> Key {
        match self {
            LedgerSubEntry::Version => Key::new(addr, KeyType::VERSION),
            LedgerSubEntry::Balance => Key::new(addr, KeyType::BALANCE),
            LedgerSubEntry::Bytecode => Key::new(addr, KeyType::BYTECODE),
            LedgerSubEntry::Datastore(hash) => Key::new(addr, KeyType::DATASTORE(hash.to_vec())),
        }
    }
}

/// Disk ledger DB module
///
/// Contains a `RocksDB` DB instance
pub struct LedgerDB {
    db: ShareableMassaDBController,
    thread_count: u8,
    key_serializer_db: KeySerializer,
    key_deserializer_db: KeyDeserializer,
    amount_serializer: AmountSerializer,
    version_serializer: U64VarIntSerializer,
    version_deserializer: U64VarIntDeserializer,
    bytecode_serializer: BytecodeSerializer,
    amount_deserializer: AmountDeserializer,
    bytecode_deserializer: BytecodeDeserializer,
    max_datastore_value_length: u64,
    max_datastore_key_length: u8,
}

impl Debug for LedgerDB {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let db = self.db.read();
        write!(f, "{:#?}", db)
    }
}

impl LedgerDB {
    /// Create and initialize a new `LedgerDB`.
    ///
    /// # Arguments
    /// * path: path to the desired disk ledger db directory
    pub fn new(
        db: ShareableMassaDBController,
        thread_count: u8,
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
    ) -> Self {
        LedgerDB {
            db,
            thread_count,
            key_serializer_db: KeySerializer::new(false),
            key_deserializer_db: KeyDeserializer::new(max_datastore_key_length, false),
            amount_serializer: AmountSerializer::new(),
            bytecode_serializer: BytecodeSerializer::new(),
            amount_deserializer: AmountDeserializer::new(
                Bound::Included(Amount::MIN),
                Bound::Included(Amount::MAX),
            ),
            bytecode_deserializer: BytecodeDeserializer::new(max_datastore_value_length),
            version_serializer: U64VarIntSerializer::new(),
            version_deserializer: U64VarIntDeserializer::new(
                Bound::Included(0),
                Bound::Included(u64::MAX),
            ),
            max_datastore_value_length,
            max_datastore_key_length,
        }
    }

    /// Loads the initial disk ledger
    ///
    /// # Arguments
    pub fn load_initial_ledger(&mut self, initial_ledger: HashMap<Address, LedgerEntry>) {
        let mut batch = DBBatch::new();

        for (address, entry) in initial_ledger {
            self.put_entry(&address, entry, &mut batch);
        }

        self.db.write().write_batch(
            batch,
            Default::default(),
            Some(Slot::new(0, self.thread_count.saturating_sub(1))),
        );
    }

    /// Allows applying `LedgerChanges` to the disk ledger
    ///
    /// # Arguments
    /// * changes: ledger changes to be applied
    /// * batch: the batch to apply the changes to
    pub fn apply_changes_to_batch(&self, changes: LedgerChanges, batch: &mut DBBatch) {
        // for all incoming changes
        for (addr, change) in changes.0 {
            match change {
                // the incoming change sets a ledger entry to a new one
                SetUpdateOrDelete::Set(new_entry) => {
                    // inserts/overwrites the entry with the incoming one
                    self.put_entry(&addr, new_entry, batch);
                }
                // the incoming change updates an existing ledger entry
                SetUpdateOrDelete::Update(entry_update) => {
                    // applies the updates to the entry
                    // if the entry does not exist, inserts a default one and applies the updates to it
                    self.update_entry(&addr, entry_update, batch);
                }
                // the incoming change deletes a ledger entry
                SetUpdateOrDelete::Delete => {
                    // delete the entry, if it exists
                    self.delete_entry(&addr, batch);
                }
            }
        }
    }

    /// Get the given sub-entry of a given address.
    ///
    /// # Arguments
    /// * `addr`: associated address
    /// * `ty`: type of the queried sub-entry
    ///
    /// # Returns
    /// An Option of the sub-entry value as bytes
    pub fn get_sub_entry(&self, addr: &Address, ty: LedgerSubEntry) -> Option<Vec<u8>> {
        let db = self.db.read();
        let key = ty.derive_key(addr);
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(&key, &mut serialized_key)
            .expect(KEY_SER_ERROR);
        db.get_cf(STATE_CF, serialized_key).expect(CRUD_ERROR)
    }

    /// Get every key of the datastore for a given address.
    ///
    /// # Returns
    /// A `BTreeSet` of the datastore keys
    pub fn get_datastore_keys(
        &self,
        addr: &Address,
        prefix: &[u8],
        start_key: Bound<Vec<u8>>,
        end_key: Bound<Vec<u8>>,
        count: Option<u32>,
    ) -> Option<BTreeSet<Vec<u8>>> {
        let db = self.db.read();

        // check if address exists, return None if it does not
        {
            let key = LedgerSubEntry::Balance.derive_key(addr);
            let mut serialized_key = Vec::new();
            self.key_serializer_db
                .serialize(&key, &mut serialized_key)
                .expect(KEY_SER_ERROR);
            db.get_cf(STATE_CF, serialized_key).expect(CRUD_ERROR)?;
        }

        // deduce datastore key iteration range
        let Some((start_bound, end_bound)) =
            range_intersection(get_prefix_bounds(prefix), (start_key, end_key))
        else {
            // empty range: no keys
            return Some(Default::default());
        };
        // translate the range bounds in terms of database keys
        let start_key = match start_bound {
            std::ops::Bound::Unbounded => datastore_prefix_from_address(addr, &[]),
            std::ops::Bound::Included(k) => datastore_prefix_from_address(addr, &k),
            std::ops::Bound::Excluded(k) => {
                let mut v = datastore_prefix_from_address(addr, &k);
                v.push(0); // get the next key, lexicographically speaking
                v
            }
        };
        let end_bound = match end_bound {
            std::ops::Bound::Unbounded => {
                get_prefix_bounds(&datastore_prefix_from_address(addr, &[])).1
            }

            std::ops::Bound::Excluded(k) => {
                std::ops::Bound::Excluded(datastore_prefix_from_address(addr, &k))
            }
            std::ops::Bound::Included(k) => {
                std::ops::Bound::Included(datastore_prefix_from_address(addr, &k))
            }
        };

        // collect the keys
        let mut res = BTreeSet::new();
        for (key, _) in db.iterator_cf(
            STATE_CF,
            MassaIteratorMode::From(&start_key, MassaDirection::Forward),
        ) {
            // check count
            if let Some(cnt) = count.as_ref() {
                if res.len() >= *cnt as usize {
                    break;
                }
            }

            // check range
            let do_continue = match &end_bound {
                std::ops::Bound::Unbounded => true,
                std::ops::Bound::Included(ub) => &key <= ub,
                std::ops::Bound::Excluded(ub) => &key < ub,
            };
            if !do_continue {
                break;
            }

            // deserialize key and check type
            let (_rest, key) = self
                .key_deserializer_db
                .deserialize::<DeserializeError>(&key)
                .expect("could not deserialize datastore key from state db");
            if let KeyType::DATASTORE(key) = key.key_type {
                res.insert(key);
            }
        }

        Some(res)
    }

    pub fn reset(&self) {
        self.db.write().delete_prefix(LEDGER_PREFIX, STATE_CF, None);
    }

    /// Deserializes the key and value, useful after bootstrap
    pub fn is_key_value_valid(&self, serialized_key: &[u8], serialized_value: &[u8]) -> bool {
        if !serialized_key.starts_with(LEDGER_PREFIX.as_bytes()) {
            return false;
        }

        let Ok((rest, key)) = self
            .key_deserializer_db
            .deserialize::<DeserializeError>(serialized_key)
        else {
            return false;
        };
        if !rest.is_empty() {
            return false;
        }

        match key.key_type {
            KeyType::VERSION => {
                let Ok((rest, _version)) = self
                    .version_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            KeyType::BALANCE => {
                let Ok((rest, _amount)) = self
                    .amount_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            KeyType::BYTECODE => {
                let Ok((rest, _bytecode)) = self
                    .bytecode_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            KeyType::DATASTORE(_) => {
                if serialized_value.len() >= self.max_datastore_value_length as usize {
                    return false;
                }
            }
        }

        true
    }
}

// Private helpers
impl LedgerDB {
    /// Add every sub-entry individually for a given entry.
    ///
    /// # Arguments
    /// * `addr`: associated address
    /// * `ledger_entry`: complete entry to be added
    /// * `batch`: the given operation batch to update
    fn put_entry(&self, addr: &Address, ledger_entry: LedgerEntry, batch: &mut DBBatch) {
        let db = self.db.read();

        // Ensures any potential previous entry is fully deleted.
        delete_datastore_entries(addr, &db, batch);

        // Version
        //TODO: Get version number from parameters
        let mut bytes_version = Vec::new();
        self.version_serializer
            .serialize(&0, &mut bytes_version)
            .unwrap();
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(&Key::new(addr, KeyType::VERSION), &mut serialized_key)
            .expect(KEY_SER_ERROR);
        db.put_or_update_entry_value(batch, serialized_key, &bytes_version);

        // Amount serialization never fails
        let mut bytes_balance = Vec::new();
        self.amount_serializer
            .serialize(&ledger_entry.balance, &mut bytes_balance)
            .unwrap();

        let mut bytes_bytecode = Vec::new();
        self.bytecode_serializer
            .serialize(&ledger_entry.bytecode, &mut bytes_bytecode)
            .unwrap();

        // balance
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(&Key::new(addr, KeyType::BALANCE), &mut serialized_key)
            .expect(KEY_SER_ERROR);
        db.put_or_update_entry_value(batch, serialized_key, &bytes_balance);

        // bytecode
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(&Key::new(addr, KeyType::BYTECODE), &mut serialized_key)
            .expect(KEY_SER_ERROR);
        db.put_or_update_entry_value(batch, serialized_key, &bytes_bytecode);

        // datastore
        for (key, entry) in ledger_entry.datastore {
            if entry.len() > self.max_datastore_value_length as usize {
                panic!(
                    "Datastore entry for address {} and key {:?} is too big ({} > {})",
                    addr,
                    key,
                    entry.len(),
                    self.max_datastore_value_length
                );
            }
            if key.len() > self.max_datastore_key_length as usize {
                panic!(
                    "Datastore key for address {} and key {:?} is too big ({} > {})",
                    addr,
                    key,
                    key.len(),
                    self.max_datastore_key_length
                );
            }
            let mut serialized_key = Vec::new();
            self.key_serializer_db
                .serialize(
                    &Key::new(addr, KeyType::DATASTORE(key)),
                    &mut serialized_key,
                )
                .expect(KEY_SER_ERROR);
            db.put_or_update_entry_value(batch, serialized_key, &entry);
        }
    }

    /// Update the ledger entry of a given address.
    ///
    /// # Arguments
    /// * `entry_update`: a descriptor of the entry updates to be applied
    /// * `batch`: the given operation batch to update
    fn update_entry(&self, addr: &Address, entry_update: LedgerEntryUpdate, batch: &mut DBBatch) {
        let db = self.db.read();

        // balance
        if let SetOrKeep::Set(balance) = entry_update.balance {
            let mut bytes = Vec::new();
            // Amount serialization never fails
            self.amount_serializer
                .serialize(&balance, &mut bytes)
                .unwrap();

            let mut serialized_key = Vec::new();
            self.key_serializer_db
                .serialize(&Key::new(addr, KeyType::BALANCE), &mut serialized_key)
                .expect(KEY_SER_ERROR);
            db.put_or_update_entry_value(batch, serialized_key, &bytes);
        }

        // bytecode
        if let SetOrKeep::Set(bytecode) = entry_update.bytecode {
            let mut bytes = Vec::new();
            self.bytecode_serializer
                .serialize(&bytecode, &mut bytes)
                .unwrap();

            let mut serialized_key = Vec::new();
            self.key_serializer_db
                .serialize(&Key::new(addr, KeyType::BYTECODE), &mut serialized_key)
                .expect(KEY_SER_ERROR);
            db.put_or_update_entry_value(batch, serialized_key, &bytes);
        }

        // datastore
        for (key, update) in entry_update.datastore {
            if key.len() > self.max_datastore_key_length as usize {
                panic!(
                    "Datastore key for address {} and key {:?} is too big ({} > {})",
                    addr,
                    key,
                    key.len(),
                    self.max_datastore_key_length
                );
            }
            let mut serialized_key = Vec::new();
            self.key_serializer_db
                .serialize(
                    &Key::new(addr, KeyType::DATASTORE(key)),
                    &mut serialized_key,
                )
                .expect(KEY_SER_ERROR);

            match update {
                SetOrDelete::Set(entry) => {
                    if entry.len() > self.max_datastore_value_length as usize {
                        panic!(
                            "Datastore entry for address {} is too big ({} > {})",
                            addr,
                            entry.len(),
                            self.max_datastore_value_length
                        );
                    } else {
                        db.put_or_update_entry_value(batch, serialized_key, &entry);
                    }
                }
                SetOrDelete::Delete => db.delete_key(batch, serialized_key),
            }
        }
    }

    /// Delete every sub-entry associated to the given address.
    ///
    /// # Arguments
    /// * batch: the given operation batch to update
    fn delete_entry(&self, addr: &Address, batch: &mut DBBatch) {
        let db = self.db.read();

        // version
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(&Key::new(addr, KeyType::VERSION), &mut serialized_key)
            .expect(KEY_SER_ERROR);
        db.delete_key(batch, serialized_key);

        // balance
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(&Key::new(addr, KeyType::BALANCE), &mut serialized_key)
            .expect(KEY_SER_ERROR);
        db.delete_key(batch, serialized_key);

        // bytecode
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(&Key::new(addr, KeyType::BYTECODE), &mut serialized_key)
            .expect(KEY_SER_ERROR);
        db.delete_key(batch, serialized_key);

        delete_datastore_entries(addr, &db, batch);
    }
}

// Helper function to delete all datastore entries for a given address
// Needs to be called in put entry and delete entry
// Note: This function takes a lock on the DB to avoid multiple reads.
fn delete_datastore_entries(
    addr: &Address,
    db: &RwLockReadGuard<RawRwLock, Box<dyn MassaDBController>>,
    batch: &mut std::collections::BTreeMap<Vec<u8>, Option<Vec<u8>>>,
) {
    // datastore
    let key_prefix = datastore_prefix_from_address(addr, &[]);
    for (serialized_key, _) in db
        .iterator_cf(
            STATE_CF,
            MassaIteratorMode::From(&key_prefix, MassaDirection::Forward),
        )
        .take_while(|(key, _)| key < &end_prefix(&key_prefix).unwrap())
    {
        db.delete_key(batch, serialized_key.to_vec());
    }
}

// test helpers
impl LedgerDB {
    /// Get every address and their corresponding balance.
    ///
    /// IMPORTANT: This should only be used for debug purposes.
    ///
    /// # Returns
    /// A `BTreeMap` with the address as key and the balance as value
    #[cfg(feature = "test-exports")]
    pub fn get_every_address(
        &self,
    ) -> std::collections::BTreeMap<Address, massa_models::amount::Amount> {
        use massa_models::address::AddressDeserializer;
        let db = self.db.write();

        let ledger = db
            .prefix_iterator_cf(STATE_CF, LEDGER_PREFIX.as_bytes())
            .take_while(|(key, _)| key.starts_with(LEDGER_PREFIX.as_bytes()))
            .collect::<Vec<_>>();

        let mut addresses = std::collections::BTreeMap::new();
        let address_deserializer = AddressDeserializer::new();
        for (key, entry) in ledger.iter() {
            let (rest, address) = address_deserializer
                .deserialize::<DeserializeError>(&key[LEDGER_PREFIX.len()..])
                .unwrap();
            if rest.first() == Some(&BALANCE_IDENT) {
                let (_, amount) = self
                    .amount_deserializer
                    .deserialize::<DeserializeError>(entry)
                    .unwrap();
                addresses.insert(address, amount);
            }
        }
        addresses
    }

    /// Get the entire datastore for a given address (no deserialization)
    ///
    /// IMPORTANT: This should only be used for debug purposes.
    #[allow(dead_code)]
    #[cfg(any(test, feature = "test-exports"))]
    pub fn get_entire_datastore_raw(
        &self,
        addr: &Address,
    ) -> std::collections::BTreeMap<Vec<u8>, Vec<u8>> {
        let db = self.db.read();

        let key_prefix = datastore_prefix_from_address(addr, &[]);
        // Need an end_prefix otherwise the iterator can return entries not belonging to the given
        // address
        let end_prefix = key_prefix[0..key_prefix.len() - 1].to_vec();

        db.iterator_cf_for_full_db_traversal(
            STATE_CF,
            MassaIteratorMode::From(&key_prefix, MassaDirection::Forward),
        )
        .take_while(|(k, _)| k.starts_with(&end_prefix))
        .collect()
    }

    /// Get the entire datastore for a given address.
    ///
    /// IMPORTANT: This should only be used for debug purposes.
    ///
    /// # Returns
    /// A `BTreeMap` with the entry hash as key and the data bytes as value
    #[cfg(any(test, feature = "test-exports"))]
    pub fn get_entire_datastore(
        &self,
        addr: &Address,
    ) -> std::collections::BTreeMap<Vec<u8>, Vec<u8>> {
        let db = self.db.read();

        let key_prefix = datastore_prefix_from_address(addr, &[]);

        db.iterator_cf_for_full_db_traversal(
            STATE_CF,
            MassaIteratorMode::From(&key_prefix, MassaDirection::Forward),
        )
        .take_while(|(k, _)| k <= &end_prefix(&key_prefix).unwrap())
        .map(|(key, data)| {
            let (_rest, key) = self
                .key_deserializer_db
                .deserialize::<DeserializeError>(&key)
                .unwrap();
            match key.key_type {
                KeyType::DATASTORE(datastore_vec) => (datastore_vec, data.to_vec()),
                _ => (vec![], vec![]),
            }
        })
        .collect()
    }
}

/// For a given start prefix (inclusive), returns the correct end prefix (non-inclusive).
/// This assumes the key bytes are ordered in lexicographical order.
/// Since key length is not limited, for some case we return `None` because there is
/// no bounded limit (every keys in the series `[]`, `[255]`, `[255, 255]` ...).
fn end_prefix(prefix: &[u8]) -> Option<Vec<u8>> {
    match get_prefix_bounds(prefix).1 {
        std::ops::Bound::Excluded(end) => Some(end),
        std::ops::Bound::Unbounded => None,
        _ => panic!("unexpected key bound"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_db_exports::{MassaDBConfig, MassaDBController, STATE_HASH_INITIAL_BYTES};
    use massa_db_worker::MassaDB;
    use massa_hash::HashXof;
    use massa_ledger_exports::{LedgerEntry, LedgerEntryUpdate};
    use massa_models::types::SetOrKeep;
    use massa_models::{
        address::Address,
        amount::{Amount, AmountDeserializer},
    };
    use massa_serialization::{DeserializeError, Deserializer};
    use massa_signature::KeyPair;
    use parking_lot::RwLock;
    use std::collections::BTreeMap;
    use std::ops::Bound::Included;
    use std::str::FromStr;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_test_ledger(addr: Address) -> (LedgerDB, DBBatch, BTreeMap<Vec<u8>, Vec<u8>>) {
        // init data

        let mut data = BTreeMap::new();
        data.insert(b"1".to_vec(), b"a".to_vec());
        data.insert(b"11".to_vec(), b"a".to_vec());
        data.insert(b"111".to_vec(), b"a".to_vec());
        data.insert(b"12".to_vec(), b"a".to_vec());
        data.insert(b"13".to_vec(), b"a".to_vec());

        data.insert(b"2".to_vec(), b"b".to_vec());
        data.insert(b"21".to_vec(), b"a".to_vec());

        data.insert(b"3".to_vec(), b"c".to_vec());
        data.insert(b"34".to_vec(), b"c".to_vec());

        let entry = LedgerEntry {
            balance: Amount::from_str("42").unwrap(),
            datastore: data.clone(),
            ..Default::default()
        };
        let entry_update = LedgerEntryUpdate {
            balance: SetOrKeep::Set(Amount::from_str("21").unwrap()),
            bytecode: SetOrKeep::Keep,
            ..Default::default()
        };

        // write data
        let temp_dir = TempDir::new().unwrap();

        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 10,
            max_final_state_elements_size: 100_000,
            max_versioning_elements_size: 100_000,
            max_ledger_backups: 10,
            thread_count: 32,
            enable_metrics: false,
        };

        let db = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
        ));

        let ledger_db = LedgerDB::new(db.clone(), 32, 255, 1000);
        let mut batch = DBBatch::new();

        ledger_db.put_entry(&addr, entry, &mut batch);
        ledger_db.update_entry(&addr, entry_update, &mut batch);
        // ledger_db
        //     .db
        //     .write()
        //     .write_batch(batch, Default::default(), None);

        // return db and initial data
        // (ledger_db, data)
        (ledger_db, batch, data)
    }

    /// Functional test of `LedgerDB`
    #[test]
    fn test_ledger_db() {
        let addr = Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());
        let (ledger_db, batch, data) = setup_test_ledger(addr);
        ledger_db
            .db
            .write()
            .write_batch(batch, Default::default(), None);

        let amount_deserializer =
            AmountDeserializer::new(Included(Amount::MIN), Included(Amount::MAX));

        // check initial state and entry update
        assert!(ledger_db
            .get_sub_entry(&addr, LedgerSubEntry::Balance)
            .is_some());
        assert!(ledger_db
            .get_sub_entry(&addr, LedgerSubEntry::Version)
            .is_some());
        assert_eq!(
            amount_deserializer
                .deserialize::<DeserializeError>(
                    &ledger_db
                        .get_sub_entry(&addr, LedgerSubEntry::Balance)
                        .unwrap()
                )
                .unwrap()
                .1,
            Amount::from_str("21").unwrap()
        );
        assert_eq!(data, ledger_db.get_entire_datastore(&addr));

        assert_ne!(
            HashXof(*STATE_HASH_INITIAL_BYTES),
            ledger_db.db.read().get_xof_db_hash()
        );

        // delete entry
        let mut batch = DBBatch::new();
        ledger_db.delete_entry(&addr, &mut batch);
        ledger_db
            .db
            .write()
            .write_batch(batch, Default::default(), None);

        // check deleted address and ledger hash
        assert_eq!(
            HashXof(*STATE_HASH_INITIAL_BYTES),
            ledger_db.db.read().get_xof_db_hash()
        );
        assert!(ledger_db
            .get_sub_entry(&addr, LedgerSubEntry::Balance)
            .is_none());
        assert!(ledger_db.get_entire_datastore(&addr).is_empty());
    }

    #[test]
    fn test_end_prefix() {
        assert_eq!(end_prefix(&[5, 6, 7]), Some(vec![5, 6, 8]));
        assert_eq!(end_prefix(&[5, 6, 255]), Some(vec![5, 7]));

        // test end prefix for address with all bytes set to 255
        {
            use massa_serialization::Deserializer;
            let addr_deser = massa_models::address::AddressDeserializer::new();
            let serialized_addr = vec![
                0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255,
            ];
            let (_, addr): (_, Address) = addr_deser
                .deserialize::<massa_serialization::DeserializeError>(&serialized_addr)
                .unwrap();
            let prefix = end_prefix(&datastore_prefix_from_address(&addr, &[]));
            assert!(prefix.is_some());
            assert_eq!(
                prefix,
                Some(vec![
                    108, 101, 100, 103, 101, 114, 47, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 4,
                ])
            );
        }
    }

    #[test]
    fn test_ledger_delete() {
        let keypair = KeyPair::generate(0).unwrap();
        let addr = Address::from_public_key(&keypair.get_public_key());
        let (ledger_db, mut batch, _data) = setup_test_ledger(addr);

        // Add a key, value into db with a prefix right after datastore (datastore indent = 3, here we use indent = 4)
        // This is to test if delete_datastore_entries only delete datastore entries
        let mut datastore_key = vec![];
        ledger_db
            .key_serializer_db
            .serialize(
                &Key::new(&addr, KeyType::DATASTORE(b"a".to_vec())),
                &mut datastore_key,
            )
            .expect(KEY_SER_ERROR);
        let datastore_key_len = datastore_key.len();
        // We modify the datastore key indent (from 3 to 4) so this is not a 'datastore' key anymore
        datastore_key[datastore_key_len - 2] = DATASTORE_IDENT + 1;
        // Value is not important here
        let datastore_value = b"a".to_vec();
        batch.insert(datastore_key.clone(), Some(datastore_value.clone()));
        //

        // Write batch into db
        ledger_db
            .db
            .write()
            .write_batch(batch, Default::default(), None);

        let datastore = ledger_db.get_entire_datastore(&addr);
        // println!("datastore: {:?}", datastore);
        assert!(!datastore.is_empty());

        let mut batch = DBBatch::new();
        let guard = ledger_db.db.read();
        delete_datastore_entries(&addr, &guard, &mut batch);
        drop(guard);

        let mut guard = ledger_db.db.write();
        guard.write_batch(batch, Default::default(), None);
        drop(guard);

        let datastore = ledger_db.get_entire_datastore_raw(&addr);
        // println!("datastore: {:?}", datastore);
        assert_eq!(datastore.len(), 1);
        assert!(datastore.contains_key(&datastore_key));
    }

    #[test]
    fn get_datastore_keys() {
        let keypair = KeyPair::generate(0).unwrap();
        let addr = Address::from_public_key(&keypair.get_public_key());
        let (ledger_db, batch, _d) = setup_test_ledger(addr);

        ledger_db
            .db
            .write()
            .write_batch(batch, Default::default(), None);

        add_noise_to_ledger(&ledger_db);

        let keys = ledger_db
            .get_datastore_keys(&addr, &[], Bound::Unbounded, Bound::Unbounded, None)
            .unwrap();

        assert_eq!(keys.len(), 9);

        let keys = ledger_db
            .get_datastore_keys(&addr, &[], Bound::Unbounded, Bound::Unbounded, Some(4))
            .unwrap();

        assert_eq!(keys.len(), 4);

        let mut keys = ledger_db
            .get_datastore_keys(
                &addr,
                &[],
                Bound::Included(b"2".to_vec()),
                Bound::Unbounded,
                None,
            )
            .unwrap();

        assert_eq!(keys.pop_first().unwrap(), b"2".to_vec());
        assert_eq!(keys.pop_first().unwrap(), b"21".to_vec());
        assert_eq!(keys.pop_first().unwrap(), b"3".to_vec());
        assert_eq!(keys.pop_first().unwrap(), b"34".to_vec());
        assert_eq!(keys.pop_first(), None);

        let mut keys = ledger_db
            .get_datastore_keys(
                &addr,
                &[],
                Bound::Included(b"2".to_vec()),
                Bound::Excluded(b"3".to_vec()),
                None,
            )
            .unwrap();

        assert_eq!(keys.pop_first().unwrap(), b"2".to_vec());
        assert_eq!(keys.pop_first().unwrap(), b"21".to_vec());
        assert_eq!(keys.pop_first(), None);

        let mut keys = ledger_db
            .get_datastore_keys(
                &addr,
                &[],
                Bound::Excluded(b"1".to_vec()),
                Bound::Included(b"12".to_vec()),
                None,
            )
            .unwrap();

        assert_eq!(keys.pop_first().unwrap(), b"11".to_vec());
        assert_eq!(keys.pop_first().unwrap(), b"111".to_vec());
        assert_eq!(keys.pop_first().unwrap(), b"12".to_vec());
        assert_eq!(keys.pop_first(), None);
    }

    fn add_noise_to_ledger(ledger_db: &LedgerDB) {
        // add a bit of garbage data into the db to ensure bounds are respected
        let mut batch = DBBatch::new();
        // should at the beginning of the db
        batch.insert(b"Agarbage".to_vec(), Some(b"garbage".to_vec()));
        // should at the end of the db
        batch.insert(b"zgarbage".to_vec(), Some(b"garbage".to_vec()));

        // create an address with all bytes set to 255
        let addr_deser = massa_models::address::AddressDeserializer::new();
        let serialized_addr = vec![
            0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        ];
        let (_, addr) = addr_deser
            .deserialize::<DeserializeError>(&serialized_addr)
            .unwrap();
        ledger_db.put_entry(&addr, LedgerEntry::default(), &mut batch);
        ledger_db.update_entry(&addr, LedgerEntryUpdate::default(), &mut batch);

        ledger_db
            .db
            .write()
            .write_batch(batch, Default::default(), None);
    }
}
