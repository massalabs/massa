//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Module to interact with the disk ledger

use massa_db::{
    DBBatch, MassaDB, CF_ERROR, CRUD_ERROR, KEY_DESER_ERROR, KEY_SER_ERROR, LEDGER_PREFIX, STATE_CF,
};
use massa_ledger_exports::*;
use massa_models::amount::AmountDeserializer;
use massa_models::bytecode::BytecodeDeserializer;
use massa_models::{
    address::Address, amount::AmountSerializer, bytecode::BytecodeSerializer, slot::Slot,
};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use parking_lot::RwLock;
use rocksdb::{Direction, IteratorMode, ReadOptions};
use std::collections::{BTreeSet, HashMap};
use std::{fmt::Debug, sync::Arc};

use massa_models::amount::Amount;
use std::ops::Bound;

/// Ledger sub entry enum
pub enum LedgerSubEntry {
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
    db: Arc<RwLock<MassaDB>>,
    thread_count: u8,
    key_serializer_db: KeySerializer,
    key_deserializer_db: KeyDeserializer,
    amount_serializer: AmountSerializer,
    bytecode_serializer: BytecodeSerializer,
    amount_deserializer: AmountDeserializer,
    bytecode_deserializer: BytecodeDeserializer,
    max_datastore_value_length: u64,
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
        db: Arc<RwLock<MassaDB>>,
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
            max_datastore_value_length,
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
            Some(Slot::new(0, self.thread_count.saturating_sub(1))),
        );
    }

    /// Allows applying `LedgerChanges` to the disk ledger
    ///
    /// # Arguments
    /// * changes: ledger changes to be applied
    /// * slot: new slot associated to the final ledger
    /// * final_state_data: the serialized final state data to include, in case we use the feature `create_snapshot`
    pub fn apply_changes_to_batch(&self, changes: LedgerChanges, ledger_batch: &mut DBBatch) {
        // for all incoming changes
        for (addr, change) in changes.0 {
            match change {
                // the incoming change sets a ledger entry to a new one
                SetUpdateOrDelete::Set(new_entry) => {
                    // inserts/overwrites the entry with the incoming one
                    self.put_entry(&addr, new_entry, ledger_batch);
                }
                // the incoming change updates an existing ledger entry
                SetUpdateOrDelete::Update(entry_update) => {
                    // applies the updates to the entry
                    // if the entry does not exist, inserts a default one and applies the updates to it
                    self.update_entry(&addr, entry_update, ledger_batch);
                }
                // the incoming change deletes a ledger entry
                SetUpdateOrDelete::Delete => {
                    // delete the entry, if it exists
                    self.delete_entry(&addr, ledger_batch);
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
        let handle = db.db.cf_handle(STATE_CF).expect(CF_ERROR);
        let key = ty.derive_key(addr);
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(&key, &mut serialized_key)
            .expect(KEY_SER_ERROR);
        db.db.get_cf(handle, serialized_key).expect(CRUD_ERROR)
    }

    /// Get every key of the datastore for a given address.
    ///
    /// # Returns
    /// A `BTreeSet` of the datastore keys
    pub fn get_datastore_keys(&self, addr: &Address) -> Option<BTreeSet<Vec<u8>>> {
        let db = self.db.read();
        let handle = db.db.cf_handle(STATE_CF).expect(CF_ERROR);

        let mut opt = ReadOptions::default();
        let key_prefix = datastore_prefix_from_address(addr);

        opt.set_iterate_range(key_prefix.clone()..end_prefix(&key_prefix).unwrap());

        let mut iter = db
            .db
            .iterator_cf_opt(handle, opt, IteratorMode::Start)
            .flatten()
            .map(|(key, _)| {
                let (_rest, key) = self
                    .key_deserializer_db
                    .deserialize::<DeserializeError>(&key)
                    .unwrap();
                match key.key_type {
                    KeyType::DATASTORE(datastore_vec) => datastore_vec,
                    _ => {
                        vec![]
                    }
                }
            })
            .peekable();

        // Return None if empty
        // TODO: function should return None if complete entry does not exist
        // and Some([]) if it does but datastore is empty
        iter.peek()?;
        Some(iter.collect())
    }

    pub fn reset(&self) {
        self.db.write().delete_prefix(LEDGER_PREFIX, None);
    }

    /// Deserializes the key and value, useful after bootstrap
    pub fn is_key_value_valid(&self, serialized_key: &[u8], serialized_value: &[u8]) -> bool {
        if !serialized_key.starts_with(LEDGER_PREFIX.as_bytes()) {
            return false;
        }

        let Ok((rest, key)) = self.key_deserializer_db.deserialize::<DeserializeError>(serialized_key) else {
            return false;
        };
        if !rest.is_empty() {
            return false;
        }

        match key.key_type {
            KeyType::BALANCE => {
                let Ok((rest, _amount)) = self.amount_deserializer.deserialize::<DeserializeError>(serialized_value) else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            KeyType::BYTECODE => {
                let Ok((rest, _bytecode)) = self.bytecode_deserializer.deserialize::<DeserializeError>(serialized_value) else {
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
    /// Internal function to put a key & value and perform the ledger hash XORs
    fn put_entry_value(&self, batch: &mut DBBatch, key: &Key, value: &[u8]) {
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(key, &mut serialized_key)
            .expect(KEY_SER_ERROR);

        self.db
            .read()
            .put_or_update_entry_value(batch, serialized_key, value);
    }

    /// Add every sub-entry individually for a given entry.
    ///
    /// # Arguments
    /// * `addr`: associated address
    /// * `ledger_entry`: complete entry to be added
    /// * `batch`: the given operation batch to update
    fn put_entry(&self, addr: &Address, ledger_entry: LedgerEntry, batch: &mut DBBatch) {
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
        self.put_entry_value(batch, &Key::new(addr, KeyType::BALANCE), &bytes_balance);

        // bytecode
        self.put_entry_value(batch, &Key::new(addr, KeyType::BYTECODE), &bytes_bytecode);

        // datastore
        for (hash, entry) in ledger_entry.datastore {
            self.put_entry_value(batch, &Key::new(addr, KeyType::DATASTORE(hash)), &entry);
        }
    }

    /// Internal function to update a key & value and perform the ledger hash XORs
    fn update_key_value(&self, batch: &mut DBBatch, key: &Key, value: &[u8]) {
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(key, &mut serialized_key)
            .expect(KEY_SER_ERROR);

        self.db
            .read()
            .put_or_update_entry_value(batch, serialized_key, value);
    }

    /// Update the ledger entry of a given address.
    ///
    /// # Arguments
    /// * `entry_update`: a descriptor of the entry updates to be applied
    /// * `batch`: the given operation batch to update
    fn update_entry(&self, addr: &Address, entry_update: LedgerEntryUpdate, batch: &mut DBBatch) {
        // balance
        if let SetOrKeep::Set(balance) = entry_update.balance {
            let mut bytes = Vec::new();
            // Amount serialization never fails
            self.amount_serializer
                .serialize(&balance, &mut bytes)
                .unwrap();

            let balance_key = Key::new(addr, KeyType::BALANCE);
            self.update_key_value(batch, &balance_key, &bytes);
        }

        // bytecode
        if let SetOrKeep::Set(bytecode) = entry_update.bytecode {
            let mut bytes = Vec::new();
            self.bytecode_serializer
                .serialize(&bytecode, &mut bytes)
                .unwrap();

            let bytecode_key = Key::new(addr, KeyType::BYTECODE);
            self.update_key_value(batch, &bytecode_key, &bytes);
        }

        // datastore
        for (hash, update) in entry_update.datastore {
            let datastore_key = Key::new(addr, KeyType::DATASTORE(hash));
            match update {
                SetOrDelete::Set(entry) => self.update_key_value(batch, &datastore_key, &entry),
                SetOrDelete::Delete => self.delete_key(batch, &datastore_key),
            }
        }
    }

    /// Internal function to delete a key and perform the ledger hash XOR
    fn delete_key(&self, batch: &mut DBBatch, key: &Key) {
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(key, &mut serialized_key)
            .expect(KEY_SER_ERROR);

        self.db.read().delete_key(batch, serialized_key);
    }

    /// Delete every sub-entry associated to the given address.
    ///
    /// # Arguments
    /// * batch: the given operation batch to update
    fn delete_entry(&self, addr: &Address, batch: &mut DBBatch) {
        let db = self.db.read();
        let handle = db.db.cf_handle(STATE_CF).expect(CF_ERROR);

        // balance
        self.delete_key(batch, &Key::new(addr, KeyType::BALANCE));

        // bytecode
        self.delete_key(batch, &Key::new(addr, KeyType::BYTECODE));

        // datastore
        let mut opt = ReadOptions::default();
        let key_prefix = datastore_prefix_from_address(addr);
        opt.set_iterate_upper_bound(end_prefix(&key_prefix).unwrap());
        for (key, _) in db
            .db
            .iterator_cf_opt(
                handle,
                opt,
                IteratorMode::From(&key_prefix, Direction::Forward),
            )
            .flatten()
        {
            let (_, deserialized_key) = self
                .key_deserializer_db
                .deserialize::<DeserializeError>(&key)
                .expect(KEY_DESER_ERROR);
            self.delete_key(batch, &deserialized_key);
        }
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
    #[cfg(any(feature = "testing"))]
    pub fn get_every_address(
        &self,
    ) -> std::collections::BTreeMap<Address, massa_models::amount::Amount> {
        use massa_models::address::AddressDeserializer;
        let db = self.db.write();

        let handle = db.db.cf_handle(STATE_CF).expect(CF_ERROR);

        let ledger = db
            .db
            .prefix_iterator_cf(handle, LEDGER_PREFIX)
            .take_while(|kv| {
                kv.clone()
                    .unwrap_or_default()
                    .0
                    .starts_with(LEDGER_PREFIX.as_bytes())
            })
            .collect::<Vec<_>>();

        let mut addresses = std::collections::BTreeMap::new();
        let address_deserializer = AddressDeserializer::new();
        for (key, entry) in ledger.iter().flatten() {
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

    /// Get the entire datastore for a given address.
    ///
    /// IMPORTANT: This should only be used for debug purposes.
    ///
    /// # Returns
    /// A `BTreeMap` with the entry hash as key and the data bytes as value
    #[cfg(any(test, feature = "testing"))]
    pub fn get_entire_datastore(
        &self,
        addr: &Address,
    ) -> std::collections::BTreeMap<Vec<u8>, Vec<u8>> {
        let db = self.db.read();

        let key_prefix = datastore_prefix_from_address(addr);
        let handle = db.db.cf_handle(STATE_CF).expect(CF_ERROR);

        let mut opt = ReadOptions::default();
        opt.set_iterate_upper_bound(end_prefix(&key_prefix).unwrap());

        db.db
            .iterator_cf_opt(
                handle,
                opt,
                IteratorMode::From(&key_prefix, Direction::Forward),
            )
            .flatten()
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

#[cfg(test)]
mod tests {
    use super::*;
    use massa_db::{MassaDB, STATE_HASH_INITIAL_BYTES};
    use massa_hash::Hash;
    use massa_ledger_exports::{LedgerEntry, LedgerEntryUpdate, SetOrKeep};
    use massa_models::{
        address::Address,
        amount::{Amount, AmountDeserializer},
    };
    use massa_serialization::{DeserializeError, Deserializer};
    use massa_signature::KeyPair;
    use std::collections::BTreeMap;
    use std::ops::Bound::Included;
    use std::str::FromStr;
    use tempfile::TempDir;

    #[cfg(test)]
    fn init_test_ledger(addr: Address) -> (LedgerDB, BTreeMap<Vec<u8>, Vec<u8>>) {
        // init data

        use massa_db::MassaDBConfig;

        let mut data = BTreeMap::new();
        data.insert(b"1".to_vec(), b"a".to_vec());
        data.insert(b"2".to_vec(), b"b".to_vec());
        data.insert(b"3".to_vec(), b"c".to_vec());
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
            thread_count: 32,
        };

        let db = Arc::new(RwLock::new(MassaDB::new(db_config)));

        let ledger_db = LedgerDB::new(db.clone(), 32, 255, 1000);
        let mut batch = DBBatch::new();

        ledger_db.put_entry(&addr, entry, &mut batch);
        ledger_db.update_entry(&addr, entry_update, &mut batch);
        ledger_db.db.write().write_batch(batch, None);

        // return db and initial data
        (ledger_db, data)
    }

    /// Functional test of `LedgerDB`
    #[test]
    fn test_ledger_db() {
        let addr = Address::from_public_key(&KeyPair::generate().get_public_key());
        let (ledger_db, data) = init_test_ledger(addr);

        let amount_deserializer =
            AmountDeserializer::new(Included(Amount::MIN), Included(Amount::MAX));

        // check initial state and entry update
        assert!(ledger_db
            .get_sub_entry(&addr, LedgerSubEntry::Balance)
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
            Hash::from_bytes(STATE_HASH_INITIAL_BYTES),
            ledger_db.db.read().get_db_hash()
        );

        // delete entry

        let mut batch = DBBatch::new();
        ledger_db.delete_entry(&addr, &mut batch);

        ledger_db.db.write().write_batch(batch, None);

        // check deleted address and ledger hash
        assert_eq!(
            Hash::from_bytes(STATE_HASH_INITIAL_BYTES),
            ledger_db.db.read().get_db_hash()
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
    }
}
