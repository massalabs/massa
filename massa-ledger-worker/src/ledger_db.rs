//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Module to interact with the disk ledger

use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_ledger_exports::*;
use massa_models::{
    address::Address,
    amount::AmountSerializer,
    bytecode::BytecodeSerializer,
    error::ModelsError,
    serialization::{VecU8Deserializer, VecU8Serializer},
    slot::{Slot, SlotDeserializer, SlotSerializer},
    streaming_step::StreamingStep,
};
use massa_serialization::{DeserializeError, Deserializer, Serializer, U64VarIntSerializer};
use nom::multi::many0;
use nom::sequence::tuple;
use rocksdb::{
    checkpoint::Checkpoint, ColumnFamily, ColumnFamilyDescriptor, Direction, IteratorMode, Options,
    ReadOptions, WriteBatch, DB,
};
use std::ops::Bound;
use std::rc::Rc;
use std::{
    collections::{BTreeSet, HashMap},
    convert::TryInto,
};
use std::{fmt::Debug, path::PathBuf, sync::Arc};

#[cfg(feature = "testing")]
use massa_models::amount::{Amount, AmountDeserializer};

const LEDGER_CF: &str = "ledger";
const METADATA_CF: &str = "metadata";
const ASYNC_POOL_CF: &str = "async_pool";
const WRONG_BATCH_TYPE_ERROR: &str = "critical: wrong batch type";
const OPEN_ERROR: &str = "critical: rocksdb open operation failed";
const CRUD_ERROR: &str = "critical: rocksdb crud operation failed";
const CF_ERROR: &str = "critical: rocksdb column family operation failed";
const LEDGER_HASH_ERROR: &str = "critical: saved ledger hash is corrupted";
const KEY_DESER_ERROR: &str = "critical: key deserialization failed";
const KEY_SER_ERROR: &str = "critical: key serialization failed";
const KEY_LEN_SER_ERROR: &str = "critical: key length serialization failed";
const SLOT_KEY: &[u8; 1] = b"s";
const LEDGER_HASH_KEY: &[u8; 1] = b"h";
const LEDGER_FINAL_STATE_KEY: &[u8; 2] = b"fs";
const LEDGER_FINAL_STATE_HASH_KEY: &[u8; 3] = b"fsh";
const LEDGER_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];
const ASYNC_POOL_HASH_KEY: &[u8; 1] = b"h";

/// Returns a new `RocksDB` instance
pub fn new_rocks_db_instance(path: PathBuf) -> DB {
    let mut db_opts = Options::default();
    db_opts.create_if_missing(true);
    db_opts.create_missing_column_families(true);

    DB::open_cf_descriptors(
        &db_opts,
        path,
        vec![
            ColumnFamilyDescriptor::new(LEDGER_CF, Options::default()),
            ColumnFamilyDescriptor::new(METADATA_CF, Options::default()),
            ColumnFamilyDescriptor::new(ASYNC_POOL_CF, Options::default()),
        ],
    )
    .expect(OPEN_ERROR)
}

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
    db: Arc<parking_lot::RwLock<DB>>,
    thread_count: u8,
    key_serializer: KeySerializer,
    key_serializer_db: KeySerializer,
    key_deserializer: KeyDeserializer,
    key_deserializer_db: KeyDeserializer,
    amount_serializer: AmountSerializer,
    bytecode_serializer: BytecodeSerializer,
    slot_serializer: SlotSerializer,
    slot_deserializer: SlotDeserializer,
    len_serializer: U64VarIntSerializer,
    ledger_part_size_message_bytes: u64,
    #[cfg(feature = "testing")]
    amount_deserializer: AmountDeserializer,
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
        db: Arc<parking_lot::RwLock<DB>>,
        thread_count: u8,
        max_datastore_key_length: u8,
        ledger_part_size_message_bytes: u64,
    ) -> Self {
        LedgerDB {
            db,
            thread_count,
            key_serializer: KeySerializer::new(true),
            key_serializer_db: KeySerializer::new(false),
            key_deserializer: KeyDeserializer::new(max_datastore_key_length, true),
            key_deserializer_db: KeyDeserializer::new(max_datastore_key_length, false),
            amount_serializer: AmountSerializer::new(),
            bytecode_serializer: BytecodeSerializer::new(),
            slot_serializer: SlotSerializer::new(),
            slot_deserializer: SlotDeserializer::new(
                (Bound::Included(u64::MIN), Bound::Included(u64::MAX)),
                (Bound::Included(0_u8), Bound::Excluded(thread_count)),
            ),
            len_serializer: U64VarIntSerializer::new(),
            ledger_part_size_message_bytes,
            #[cfg(feature = "testing")]
            amount_deserializer: AmountDeserializer::new(
                Bound::Included(Amount::MIN),
                Bound::Included(Amount::MAX),
            ),
        }
    }

    pub fn set_initial_slot(&mut self, slot: Slot) {
        let ledger_hash = self.get_ledger_hash();
        let mut batch = LedgerBatch::new(Some(ledger_hash), None);
        self.set_slot(slot, &mut batch);
        self.write_batch(batch);
    }

    /// Loads the initial disk ledger
    ///
    /// # Arguments
    pub fn load_initial_ledger(&mut self, initial_ledger: HashMap<Address, LedgerEntry>) {
        // initial ledger_hash value to avoid matching an option in every XOR operation
        // because of a one time case being an empty ledger
        let ledger_hash = Hash::from_bytes(LEDGER_HASH_INITIAL_BYTES);
        let mut batch = LedgerBatch::new(Some(ledger_hash), None);
        for (address, entry) in initial_ledger {
            self.put_entry(&address, entry, &mut batch);
        }
        self.set_slot(
            Slot::new(0, self.thread_count.saturating_sub(1)),
            &mut batch,
        );
        self.write_batch(batch);
    }

    pub fn backup_db(&self, slot: Slot) {
        let mut subpath = String::from("backup_");
        subpath.push_str(slot.period.to_string().as_str());
        subpath.push('_');
        subpath.push_str(slot.thread.to_string().as_str());

        let db = self.db.read();

        Checkpoint::new(&db)
            .expect("Cannot init checkpoint")
            .create_checkpoint(db.path().join(subpath))
            .expect("Failed to create checkpoint");
    }

    /// Allows applying `LedgerChanges` to the disk ledger
    ///
    /// # Arguments
    /// * changes: ledger changes to be applied
    /// * slot: new slot associated to the final ledger
    /// * final_state_data: the serialized final state data to include, in case we use the feature `create_snapshot`
    pub fn apply_changes(&mut self, changes: LedgerChanges, slot: Slot) {
        // create the batch
        let mut batch = LedgerBatch::new(Some(self.get_ledger_hash()), None);
        // for all incoming changes
        for (addr, change) in changes.0 {
            match change {
                // the incoming change sets a ledger entry to a new one
                SetUpdateOrDelete::Set(new_entry) => {
                    // inserts/overwrites the entry with the incoming one
                    self.put_entry(&addr, new_entry, &mut batch);
                }
                // the incoming change updates an existing ledger entry
                SetUpdateOrDelete::Update(entry_update) => {
                    // applies the updates to the entry
                    // if the entry does not exist, inserts a default one and applies the updates to it
                    self.update_entry(&addr, entry_update, &mut batch);
                }
                // the incoming change deletes a ledger entry
                SetUpdateOrDelete::Delete => {
                    // delete the entry, if it exists
                    self.delete_entry(&addr, &mut batch);
                }
            }
        }
        // set the associated slot in metadata
        self.set_slot(slot, &mut batch);

        // write the batch
        self.write_batch(batch);
    }

    /// Allows applying `LedgerChanges` to the disk ledger
    ///
    /// # Arguments
    /// * changes: ledger changes to be applied
    /// * slot: new slot associated to the final ledger
    /// * final_state_data: the serialized final state data to include, in case we use the feature `create_snapshot`
    pub fn apply_changes_to_batch(
        &mut self,
        changes: LedgerChanges,
        slot: Slot,
        ledger_batch: &mut LedgerBatch,
    ) {
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
        // set the associated slot in metadata
        self.set_slot(slot, ledger_batch);
    }

    /// Get the current disk ledger hash
    pub fn get_ledger_hash(&self) -> Hash {
        let db = self.db.read();
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);
        if let Some(ledger_hash_bytes) = db
            .get_cf(handle, LEDGER_HASH_KEY)
            .expect(CRUD_ERROR)
            .as_deref()
        {
            Hash::from_bytes(ledger_hash_bytes.try_into().expect(LEDGER_HASH_ERROR))
        } else {
            // initial ledger_hash value to avoid matching an option in every XOR operation
            // because of a one time case being an empty ledger
            // also note that the if you XOR a hash with itself result is LEDGER_HASH_INITIAL_BYTES
            Hash::from_bytes(LEDGER_HASH_INITIAL_BYTES)
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
        let handle = db.cf_handle(LEDGER_CF).expect(CF_ERROR);
        let key = ty.derive_key(addr);
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(&key, &mut serialized_key)
            .expect(KEY_SER_ERROR);
        db.get_cf(handle, serialized_key).expect(CRUD_ERROR)
    }

    /// Get every key of the datastore for a given address.
    ///
    /// # Returns
    /// A `BTreeSet` of the datastore keys
    pub fn get_datastore_keys(&self, addr: &Address) -> Option<BTreeSet<Vec<u8>>> {
        let db = self.db.read();
        let handle = db.cf_handle(LEDGER_CF).expect(CF_ERROR);

        let mut opt = ReadOptions::default();
        let key_prefix = datastore_prefix_from_address(addr);

        opt.set_iterate_range(key_prefix.clone()..end_prefix(&key_prefix).unwrap());

        let mut iter = db
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

    /// Get a part of the disk Ledger.
    /// Mainly used in the bootstrap process.
    ///
    /// # Arguments
    /// * `last_key`: key where the part retrieving must start
    ///
    /// # Returns
    /// A tuple containing:
    /// * The ledger part as bytes
    /// * The last taken key (this is an optimization to easily keep a reference to the last key)
    pub fn get_ledger_part(
        &self,
        cursor: StreamingStep<Key>,
    ) -> Result<(Vec<u8>, StreamingStep<Key>), ModelsError> {
        let db = self.db.read();
        let handle = db.cf_handle(LEDGER_CF).expect(CF_ERROR);
        let opt = ReadOptions::default();
        let ser = VecU8Serializer::new();
        let mut ledger_part = Vec::new();

        // Creates an iterator from the next element after the last if defined, otherwise initialize it at the first key of the ledger.
        let (db_iterator, mut new_cursor) = match cursor {
            StreamingStep::Started => (
                db.iterator_cf_opt(handle, opt, IteratorMode::Start),
                StreamingStep::<Key>::Started,
            ),
            StreamingStep::Ongoing(last_key) => {
                let mut serialized_key = Vec::new();
                self.key_serializer_db
                    .serialize(&last_key, &mut serialized_key)?;
                let mut iter = db.iterator_cf_opt(
                    handle,
                    opt,
                    IteratorMode::From(&serialized_key, Direction::Forward),
                );
                iter.next();
                (iter, StreamingStep::Finished(None))
            }
            StreamingStep::<Key>::Finished(_) => return Ok((ledger_part, cursor)),
        };

        // Iterates over the whole database
        for (key, entry) in db_iterator.flatten() {
            if (ledger_part.len() as u64) < (self.ledger_part_size_message_bytes) {
                // We deserialize and re-serialize the key to change the key format from the
                // database one to a format we can use outside of the ledger.
                let (_, key) = self.key_deserializer_db.deserialize(&key)?;
                self.key_serializer.serialize(&key, &mut ledger_part)?;
                ser.serialize(&entry.to_vec(), &mut ledger_part)?;
                new_cursor = StreamingStep::Ongoing(key);
            } else {
                break;
            }
        }
        Ok((ledger_part, new_cursor))
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
    pub fn set_ledger_part<'a>(&self, data: &'a [u8]) -> Result<StreamingStep<Key>, ModelsError> {
        let db = self.db.read();
        let handle = db.cf_handle(LEDGER_CF).expect(CF_ERROR);
        let vec_u8_deserializer =
            VecU8Deserializer::new(Bound::Included(0), Bound::Excluded(u64::MAX));
        let mut last_key: Rc<Option<Key>> = Rc::new(None);
        let mut batch = LedgerBatch::new(Some(self.get_ledger_hash()), None);
        // Since this data is coming from the network, deser to address and ser back to bytes for a security check.
        let (rest, _) = many0(|input: &'a [u8]| {
            let (rest, (key, value)) = tuple((
                |input| self.key_deserializer.deserialize(input),
                |input| vec_u8_deserializer.deserialize(input),
            ))(input)?;
            *Rc::get_mut(&mut last_key).ok_or_else(|| {
                nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Fail))
            })? = Some(key.clone());
            self.put_entry_value(handle, &mut batch, &key, &value);
            Ok((rest, ()))
        })(data)
        .map_err(|_| ModelsError::SerializeError("Error in deserialization".to_string()))?;

        match last_key.as_ref() {
            Some(last_key) => {
                if rest.is_empty() {
                    self.write_batch(batch);
                    Ok(StreamingStep::Ongoing(last_key.clone()))
                } else {
                    Err(ModelsError::SerializeError(
                        "Error in deserialization".to_string(),
                    ))
                }
            }
            None => Ok(StreamingStep::Finished(None)),
        }
    }

    pub fn reset(&mut self) {
        let mut db = self.db.write();
        (*db).drop_cf(LEDGER_CF).expect("Error dropping ledger cf");
        (*db)
            .drop_cf(METADATA_CF)
            .expect("Error dropping metadata cf");
        let mut db_opts = Options::default();
        db_opts.set_error_if_exists(true);
        (*db)
            .create_cf(LEDGER_CF, &db_opts)
            .expect("Error creating ledger cf");
        (*db)
            .create_cf(METADATA_CF, &db_opts)
            .expect("Error creating metadata cf");
    }

    pub fn set_final_state_hash(&mut self, data: &[u8]) {
        let db = self.db.read();
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);
        let mut batch = WriteBatch::default();

        batch.put_cf(handle, LEDGER_FINAL_STATE_HASH_KEY, data);
        db.write(batch).expect(CRUD_ERROR);
    }

    pub fn get_final_state(&self) -> Result<Vec<u8>, ModelsError> {
        let db = self.db.read();
        let handle = db.cf_handle(LEDGER_CF).expect(CF_ERROR);
        let opt = ReadOptions::default();

        let Ok(Some(final_state_data)) = db.get_cf_opt(handle, LEDGER_FINAL_STATE_KEY, &opt) else {
            return Err(ModelsError::BufferError(String::from("Could not recover final_state_data")));
        };
        let Ok(Some(final_state_hash)) = db.get_pinned_cf_opt(handle, LEDGER_FINAL_STATE_HASH_KEY, &opt) else {
            return Err(ModelsError::BufferError(String::from("Could not recover final_state_hash")));
        };

        let mut final_state = final_state_data;
        final_state.extend_from_slice(&final_state_hash);

        Ok(final_state)
    }

    /// Apply the given operation batch to the disk ledger
    pub fn write_batch(&self, mut batch: LedgerBatch) {
        let db = self.db.read();
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);
        if let Some(ledger_hash) = batch.ledger_hash {
            batch
                .write_batch
                .put_cf(handle, LEDGER_HASH_KEY, ledger_hash.to_bytes());
        }
        if let Some(async_pool_hash) = batch.async_pool_hash {
            batch
                .write_batch
                .put_cf(handle, ASYNC_POOL_HASH_KEY, async_pool_hash.to_bytes());
        }
        db.write(batch.write_batch).expect(CRUD_ERROR);
    }
}

// Private helpers
impl LedgerDB {
    /// Set the disk ledger slot metadata
    ///
    /// # Arguments
    /// * slot: associated slot of the current ledger
    /// * batch: the given operation batch to update
    fn set_slot(&self, slot: Slot, batch: &mut LedgerBatch) {
        let db = self.db.read();
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);
        let mut slot_bytes = Vec::new();
        // Slot serialization never fails
        self.slot_serializer
            .serialize(&slot, &mut slot_bytes)
            .unwrap();
        batch
            .write_batch
            .put_cf(handle, SLOT_KEY, slot_bytes.clone());
        // XOR previous slot and new one
        if let Some(prev_bytes) = db.get_pinned_cf(handle, SLOT_KEY).expect(CRUD_ERROR) {
            *batch.ledger_hash.as_mut().expect(WRONG_BATCH_TYPE_ERROR) ^=
                Hash::compute_from(&prev_bytes);
        }
        *batch.ledger_hash.as_mut().expect(WRONG_BATCH_TYPE_ERROR) ^=
            Hash::compute_from(&slot_bytes);
    }

    pub fn get_slot(&self) -> Result<Slot, ModelsError> {
        let db = self.db.read();
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);

        let Ok(Some(slot_bytes)) = db.get_pinned_cf(handle, SLOT_KEY) else {
            return Err(ModelsError::BufferError(String::from("Could not recover final_state_hash")));
        };

        let (_rest, slot) = self.slot_deserializer.deserialize(&slot_bytes)?;

        Ok(slot)
    }

    /// Internal function to put a key & value and perform the ledger hash XORs
    fn put_entry_value(
        &self,
        handle: &ColumnFamily,
        batch: &mut LedgerBatch,
        key: &Key,
        value: &[u8],
    ) {
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(key, &mut serialized_key)
            .expect(KEY_SER_ERROR);
        let mut len_bytes = Vec::new();
        self.len_serializer
            .serialize(&(serialized_key.len() as u64), &mut len_bytes)
            .expect(KEY_LEN_SER_ERROR);
        let hash = Hash::compute_from(&[&len_bytes, &serialized_key, value].concat());
        *batch.ledger_hash.as_mut().expect(WRONG_BATCH_TYPE_ERROR) ^= hash;
        batch.aeh_list.insert(serialized_key.clone(), hash);
        batch.write_batch.put_cf(handle, serialized_key, value);
    }

    /// Add every sub-entry individually for a given entry.
    ///
    /// # Arguments
    /// * `addr`: associated address
    /// * `ledger_entry`: complete entry to be added
    /// * `batch`: the given operation batch to update
    fn put_entry(&mut self, addr: &Address, ledger_entry: LedgerEntry, batch: &mut LedgerBatch) {
        let db = self.db.read();
        let handle = db.cf_handle(LEDGER_CF).expect(CF_ERROR);
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
        self.put_entry_value(
            handle,
            batch,
            &Key::new(addr, KeyType::BALANCE),
            &bytes_balance,
        );

        // bytecode
        self.put_entry_value(
            handle,
            batch,
            &Key::new(addr, KeyType::BYTECODE),
            &bytes_bytecode,
        );

        // datastore
        for (hash, entry) in ledger_entry.datastore {
            self.put_entry_value(
                handle,
                batch,
                &Key::new(addr, KeyType::DATASTORE(hash)),
                &entry,
            );
        }
    }

    /// Internal function to update a key & value and perform the ledger hash XORs
    fn update_key_value(
        &self,
        handle: &ColumnFamily,
        batch: &mut LedgerBatch,
        key: &Key,
        value: &[u8],
    ) {
        let db = self.db.read();
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(key, &mut serialized_key)
            .expect(KEY_SER_ERROR);

        let mut len_bytes = Vec::new();
        self.len_serializer
            .serialize(&(serialized_key.len() as u64), &mut len_bytes)
            .expect(KEY_LEN_SER_ERROR);
        if let Some(added_hash) = batch.aeh_list.get(&serialized_key) {
            *batch.ledger_hash.as_mut().expect(WRONG_BATCH_TYPE_ERROR) ^= *added_hash;
        } else if let Some(prev_bytes) =
            db.get_pinned_cf(handle, &serialized_key).expect(CRUD_ERROR)
        {
            *batch.ledger_hash.as_mut().expect(WRONG_BATCH_TYPE_ERROR) ^=
                Hash::compute_from(&[&len_bytes, &serialized_key, &prev_bytes[..]].concat());
        }
        let hash = Hash::compute_from(&[&len_bytes, &serialized_key, value].concat());
        *batch.ledger_hash.as_mut().expect(WRONG_BATCH_TYPE_ERROR) ^= hash;
        batch.aeh_list.insert(serialized_key.clone(), hash);
        batch.write_batch.put_cf(handle, serialized_key, value);
    }

    /// Update the ledger entry of a given address.
    ///
    /// # Arguments
    /// * `entry_update`: a descriptor of the entry updates to be applied
    /// * `batch`: the given operation batch to update
    fn update_entry(
        &mut self,
        addr: &Address,
        entry_update: LedgerEntryUpdate,
        batch: &mut LedgerBatch,
    ) {
        let db = self.db.read();
        let handle = db.cf_handle(LEDGER_CF).expect(CF_ERROR);

        // balance
        if let SetOrKeep::Set(balance) = entry_update.balance {
            let mut bytes = Vec::new();
            // Amount serialization never fails
            self.amount_serializer
                .serialize(&balance, &mut bytes)
                .unwrap();

            let balance_key = Key::new(addr, KeyType::BALANCE);
            self.update_key_value(handle, batch, &balance_key, &bytes);
        }

        // bytecode
        if let SetOrKeep::Set(bytecode) = entry_update.bytecode {
            let mut bytes = Vec::new();
            self.bytecode_serializer
                .serialize(&bytecode, &mut bytes)
                .unwrap();

            let bytecode_key = Key::new(addr, KeyType::BYTECODE);
            self.update_key_value(handle, batch, &bytecode_key, &bytes);
        }

        // datastore
        for (hash, update) in entry_update.datastore {
            let datastore_key = Key::new(addr, KeyType::DATASTORE(hash));
            match update {
                SetOrDelete::Set(entry) => {
                    self.update_key_value(handle, batch, &datastore_key, &entry)
                }
                SetOrDelete::Delete => self.delete_key(handle, batch, &datastore_key),
            }
        }
    }

    /// Internal function to delete a key and perform the ledger hash XOR
    fn delete_key(&self, handle: &ColumnFamily, batch: &mut LedgerBatch, key: &Key) {
        let db = self.db.read();
        let mut serialized_key = Vec::new();
        self.key_serializer_db
            .serialize(key, &mut serialized_key)
            .expect(KEY_SER_ERROR);
        if let Some(added_hash) = batch.aeh_list.get(&serialized_key) {
            *batch.ledger_hash.as_mut().expect(WRONG_BATCH_TYPE_ERROR) ^= *added_hash;
        } else if let Some(prev_bytes) =
            db.get_pinned_cf(handle, &serialized_key).expect(CRUD_ERROR)
        {
            let mut len_bytes = Vec::new();
            self.len_serializer
                .serialize(&(serialized_key.len() as u64), &mut len_bytes)
                .expect(KEY_LEN_SER_ERROR);
            *batch.ledger_hash.as_mut().expect(WRONG_BATCH_TYPE_ERROR) ^=
                Hash::compute_from(&[&len_bytes, &serialized_key, &prev_bytes[..]].concat());
        }
        batch.write_batch.delete_cf(handle, serialized_key);
    }

    /// Delete every sub-entry associated to the given address.
    ///
    /// # Arguments
    /// * batch: the given operation batch to update
    fn delete_entry(&self, addr: &Address, batch: &mut LedgerBatch) {
        let db = self.db.read();
        let handle = db.cf_handle(LEDGER_CF).expect(CF_ERROR);

        // balance
        self.delete_key(handle, batch, &Key::new(addr, KeyType::BALANCE));

        // bytecode
        self.delete_key(handle, batch, &Key::new(addr, KeyType::BYTECODE));

        // datastore
        let mut opt = ReadOptions::default();
        let key_prefix = datastore_prefix_from_address(addr);
        opt.set_iterate_upper_bound(end_prefix(&key_prefix).unwrap());
        for (key, _) in db
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
            self.delete_key(handle, batch, &deserialized_key);
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

        let handle = db.cf_handle(LEDGER_CF).expect(CF_ERROR);

        let ledger = db
            .iterator_cf(handle, IteratorMode::Start)
            .collect::<Vec<_>>();

        let mut addresses = std::collections::BTreeMap::new();
        let address_deserializer = AddressDeserializer::new();
        for (key, entry) in ledger.iter().flatten() {
            let (rest, address) = address_deserializer
                .deserialize::<DeserializeError>(&key[..])
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
        let handle = db.cf_handle(LEDGER_CF).expect(CF_ERROR);

        let mut opt = ReadOptions::default();
        opt.set_iterate_upper_bound(end_prefix(&key_prefix).unwrap());

        db.iterator_cf_opt(
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
    use massa_hash::Hash;
    use massa_ledger_exports::{LedgerEntry, LedgerEntryUpdate, SetOrKeep};
    use massa_models::{
        address::Address,
        amount::{Amount, AmountDeserializer},
        streaming_step::StreamingStep,
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

        use parking_lot::RwLock;
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

        let rocks_db_instance = new_rocks_db_instance(temp_dir.path().to_path_buf());

        let mut db = LedgerDB::new(Arc::new(RwLock::new(rocks_db_instance)), 32, 255, 1_000_000);
        let mut batch = LedgerBatch::new(Some(Hash::from_bytes(LEDGER_HASH_INITIAL_BYTES)), None);
        db.put_entry(&addr, entry, &mut batch);
        db.update_entry(&addr, entry_update, &mut batch);
        db.write_batch(batch);

        // return db and initial data
        (db, data)
    }

    /// Functional test of `LedgerDB`
    #[test]
    fn test_ledger_db() {
        let addr = Address::from_public_key(&KeyPair::generate().get_public_key());
        let (db, data) = init_test_ledger(addr);

        let ledger_hash = db.get_ledger_hash();
        let amount_deserializer =
            AmountDeserializer::new(Included(Amount::MIN), Included(Amount::MAX));

        // check initial state and entry update
        assert!(db.get_sub_entry(&addr, LedgerSubEntry::Balance).is_some());
        assert_eq!(
            amount_deserializer
                .deserialize::<DeserializeError>(
                    &db.get_sub_entry(&addr, LedgerSubEntry::Balance).unwrap()
                )
                .unwrap()
                .1,
            Amount::from_str("21").unwrap()
        );
        assert_eq!(data, db.get_entire_datastore(&addr));
        assert_ne!(
            Hash::from_bytes(LEDGER_HASH_INITIAL_BYTES),
            db.get_ledger_hash()
        );

        // delete entry
        let mut batch = LedgerBatch::new(Some(ledger_hash), None);
        db.delete_entry(&addr, &mut batch);
        db.write_batch(batch);

        // check deleted address and ledger hash
        assert_eq!(
            Hash::from_bytes(LEDGER_HASH_INITIAL_BYTES),
            db.get_ledger_hash()
        );
        assert!(db.get_sub_entry(&addr, LedgerSubEntry::Balance).is_none());
        assert!(db.get_entire_datastore(&addr).is_empty());
    }

    #[test]
    fn test_ledger_parts() {
        let pub_a = KeyPair::generate().get_public_key();
        let a = Address::from_public_key(&pub_a);
        let (db, _) = init_test_ledger(a);
        let res = db.get_ledger_part(StreamingStep::Started).unwrap();
        db.set_ledger_part(&res.0[..]).unwrap();
    }

    #[test]
    fn test_end_prefix() {
        assert_eq!(end_prefix(&[5, 6, 7]), Some(vec![5, 6, 8]));
        assert_eq!(end_prefix(&[5, 6, 255]), Some(vec![5, 7]));
    }
}
