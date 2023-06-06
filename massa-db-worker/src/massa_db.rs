use lsmtree::{bytes::Bytes, BadProof, KVStore, SparseMerkleTree};
use massa_db_exports::{
    DBBatch, Key, MassaDBConfig, MassaDBController, MassaDBError, MassaDirection,
    MassaIteratorMode, StreamBatch, Value, CF_ERROR, CHANGE_ID_DESER_ERROR, CHANGE_ID_KEY,
    CHANGE_ID_SER_ERROR, CRUD_ERROR, LSMTREE_ERROR, LSMTREE_NODES_CF, LSMTREE_VALUES_CF,
    METADATA_CF, OPEN_ERROR, STATE_CF, STATE_HASH_ERROR, STATE_HASH_INITIAL_BYTES, STATE_HASH_KEY,
    VERSIONING_CF,
};
use massa_hash::{Hash, SmtHasher};
use massa_models::{
    error::ModelsError,
    slot::{Slot, SlotDeserializer, SlotSerializer},
    streaming_step::StreamingStep,
};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use parking_lot::{Mutex, RwLock};
use rocksdb::{
    checkpoint::Checkpoint, ColumnFamilyDescriptor, Direction, IteratorMode, Options, WriteBatch,
    DB,
};
use std::{
    collections::{BTreeMap, HashMap},
    format,
    ops::Bound::{self, Excluded, Included},
    sync::Arc,
};

/// Wrapped RocksDB database
///
/// In our instance, we use Slot as the ChangeID
pub type MassaDB = RawMassaDB<Slot, SlotSerializer, SlotDeserializer>;

/// A generic wrapped RocksDB database.
///
/// The added features are:
/// - Hash tracking with Lsm-tree, a Sparse Merkle Tree implementation
/// - Streaming the database while it is being actively updated
#[derive()]
pub struct RawMassaDB<
    ChangeID: PartialOrd + Ord + PartialEq + Eq + Clone + std::fmt::Debug,
    ChangeIDSerializer: Serializer<ChangeID>,
    ChangeIDDeserializer: Deserializer<ChangeID>,
> {
    /// The rocksdb instance
    pub db: Arc<DB>,
    /// configuration for the `RawMassaDB`
    config: MassaDBConfig,
    /// In change_history, we keep the latest changes made to the database, useful for streaming them to a client.
    pub change_history: BTreeMap<ChangeID, BTreeMap<Key, Option<Value>>>,
    /// same as change_history but for versioning
    pub change_history_versioning: BTreeMap<ChangeID, BTreeMap<Key, Option<Value>>>,
    /// A serializer for the ChangeID type
    change_id_serializer: ChangeIDSerializer,
    /// A deserializer for the ChangeID type
    change_id_deserializer: ChangeIDDeserializer,
    /// The Sparse Merkle Tree instance used to keep track of the global hash of the database
    lsmtree: SparseMerkleTree<MassaDbLsmtree>,
    /// The current RocksDB batch of the database, in a Mutex to share it with lsmtree
    current_batch: Arc<Mutex<WriteBatch>>,
    /// The current RocksDB cache for this batch, useful for lsmtree
    current_hashmap: SharedSmtCache,
}

type SharedSmtCache = Arc<RwLock<HashMap<[u8; 32], Option<Bytes>>>>;

/// Wrapper for the Lsm-tree database type
struct MassaDbLsmtree {
    pub cf: &'static str,
    pub db: Arc<DB>,
    pub current_batch: Arc<Mutex<WriteBatch>>,
    pub current_hashmap: SharedSmtCache,
}

impl MassaDbLsmtree {
    /// Constructor for `MassaDbLsmtree`
    pub fn new(
        cf: &'static str,
        db: Arc<DB>,
        current_batch: Arc<Mutex<WriteBatch>>,
        current_hashmap: SharedSmtCache,
    ) -> Self {
        Self {
            cf,
            db,
            current_batch,
            current_hashmap,
        }
    }
}

/// Implementation of the Database trait of Lsm-tree for our wrapper.
impl KVStore for MassaDbLsmtree {
    type Hasher = SmtHasher;
    type Error = BadProof;

    /// Get a value from the database, prioritizing the cache
    fn get(&self, key: &[u8]) -> Result<Option<Bytes>, Self::Error> {
        let key: [u8; 32] = key.try_into().expect(LSMTREE_ERROR);
        if let Some(val) = self.current_hashmap.read().get(&key) {
            return Ok(val.clone());
        }
        let handle_lsmtree = self.db.cf_handle(self.cf).expect(CF_ERROR);
        let value = self
            .db
            .get_cf(handle_lsmtree, key)
            .expect(CRUD_ERROR)
            .map(Bytes::from);
        self.current_hashmap.write().insert(key, value.clone());
        Ok(value)
    }

    /// Set a value to the database (in a batch), and updating the cache
    fn set(&mut self, key: Bytes, value: Bytes) -> Result<(), Self::Error> {
        let key: [u8; 32] = key.to_vec().try_into().expect(LSMTREE_ERROR);
        let handle_lsmtree = self.db.cf_handle(self.cf).expect(CF_ERROR);
        self.current_batch
            .lock()
            .put_cf(handle_lsmtree, key, value.clone());
        self.current_hashmap.write().insert(key, Some(value));
        Ok(())
    }

    /// Remove a value from the database (in a batch), and updating the cache
    fn remove(&mut self, key: &[u8]) -> Result<Bytes, Self::Error> {
        let key: [u8; 32] = key.to_vec().try_into().expect(LSMTREE_ERROR);
        let handle_lsmtree = self.db.cf_handle(self.cf).expect(CF_ERROR);
        let val = self.get(&key)?.expect(LSMTREE_ERROR);
        self.current_batch.lock().delete_cf(handle_lsmtree, key);
        self.current_hashmap.write().insert(key, None);
        Ok(val)
    }

    /// Check if a key is in the database
    fn contains(&self, key: &[u8]) -> Result<bool, Self::Error> {
        let key: [u8; 32] = key.try_into().expect(LSMTREE_ERROR);
        Ok(self.get(&key)?.is_some())
    }
}

impl<ChangeID, ChangeIDSerializer, ChangeIDDeserializer> std::fmt::Debug
    for RawMassaDB<ChangeID, ChangeIDSerializer, ChangeIDDeserializer>
where
    ChangeID: PartialOrd + Ord + PartialEq + Eq + Clone + std::fmt::Debug,
    ChangeIDSerializer: Serializer<ChangeID>,
    ChangeIDDeserializer: Deserializer<ChangeID>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawMassaDB")
            .field("db", &self.db)
            .field("config", &self.config)
            .field("change_history", &self.change_history)
            .finish()
    }
}

impl<ChangeID, ChangeIDSerializer, ChangeIDDeserializer>
    RawMassaDB<ChangeID, ChangeIDSerializer, ChangeIDDeserializer>
where
    ChangeID: PartialOrd + Ord + PartialEq + Eq + Clone + std::fmt::Debug,
    ChangeIDSerializer: Serializer<ChangeID>,
    ChangeIDDeserializer: Deserializer<ChangeID>,
{
    /// Used for bootstrap servers (get a new batch of data from STATE_CF to stream to the client)
    ///
    /// Returns a StreamBatch<ChangeID>
    pub fn get_batch_to_stream(
        &self,
        last_obtained: Option<(Vec<u8>, ChangeID)>,
    ) -> Result<StreamBatch<ChangeID>, MassaDBError> {
        let (updates_on_previous_elements, max_key) = if let Some((max_key, last_change_id)) =
            last_obtained
        {
            match last_change_id.cmp(&self.get_change_id().expect(CHANGE_ID_DESER_ERROR)) {
                std::cmp::Ordering::Greater => {
                    return Err(MassaDBError::TimeError(String::from(
                        "we don't have this change yet on this node (it's in the future for us)",
                    )));
                }
                std::cmp::Ordering::Equal => {
                    (BTreeMap::new(), Some(max_key)) // no new updates
                }
                std::cmp::Ordering::Less => {
                    // We should send all the new updates since last_change_id

                    let cursor = self
                        .change_history
                        .lower_bound(Bound::Excluded(&last_change_id));

                    if cursor.peek_prev().is_none() {
                        return Err(MassaDBError::TimeError(String::from(
                            "all our changes are strictly after last_change_id, we can't be sure we did not miss any",
                        )));
                    }

                    match cursor.key() {
                        Some(cursor_change_id) => {
                            // We have to send all the updates since cursor_change_id
                            // TODO_PR: check if / how we want to limit the number of updates we send. It may be needed but tricky to implement.
                            let mut updates: BTreeMap<Vec<u8>, Option<Vec<u8>>> = BTreeMap::new();
                            let iter = self
                                .change_history
                                .range((Bound::Included(cursor_change_id), Bound::Unbounded));
                            for (_change_id, changes) in iter {
                                updates.extend(
                                    changes
                                        .range((
                                            Bound::<Vec<u8>>::Unbounded,
                                            Included(max_key.clone()),
                                        ))
                                        .map(|(k, v)| (k.clone(), v.clone())),
                                );
                            }
                            (updates, Some(max_key))
                        }
                        None => (BTreeMap::new(), Some(max_key)), // no new updates
                    }
                }
            }
        } else {
            (BTreeMap::new(), None) // we start from the beginning, so no updates on previous elements
        };

        let mut new_elements = BTreeMap::new();
        let handle = self.db.cf_handle(STATE_CF).expect(CF_ERROR);

        // Creates an iterator from the next element after the last if defined, otherwise initialize it at the first key.
        let db_iterator = match max_key {
            None => self.db.iterator_cf(handle, IteratorMode::Start),
            Some(max_key) => {
                let mut iter = self
                    .db
                    .iterator_cf(handle, IteratorMode::From(&max_key, Direction::Forward));
                iter.next();
                iter
            }
        };

        for (serialized_key, serialized_value) in db_iterator.flatten() {
            if new_elements.len() < self.config.max_new_elements {
                new_elements.insert(serialized_key.to_vec(), serialized_value.to_vec());
            } else {
                break;
            }
        }

        Ok(StreamBatch {
            new_elements,
            updates_on_previous_elements,
            change_id: self.get_change_id().expect(CHANGE_ID_DESER_ERROR),
        })
    }

    /// Used for bootstrap servers (get a new batch of data from VERSIONING_CF to stream to the client)
    ///
    /// Returns a StreamBatch<ChangeID>
    pub fn get_versioning_batch_to_stream(
        &self,
        last_obtained: Option<(Vec<u8>, ChangeID)>,
    ) -> Result<StreamBatch<ChangeID>, MassaDBError> {
        let (updates_on_previous_elements, max_key) = if let Some((max_key, last_change_id)) =
            last_obtained
        {
            match last_change_id.cmp(&self.get_change_id().expect(CHANGE_ID_DESER_ERROR)) {
                std::cmp::Ordering::Greater => {
                    return Err(MassaDBError::TimeError(String::from(
                        "we don't have this change yet on this node (it's in the future for us)",
                    )));
                }
                std::cmp::Ordering::Equal => {
                    (BTreeMap::new(), Some(max_key)) // no new updates
                }
                std::cmp::Ordering::Less => {
                    // We should send all the new updates since last_change_id

                    let cursor = self
                        .change_history
                        .lower_bound(Bound::Excluded(&last_change_id));

                    if cursor.peek_prev().is_none() {
                        return Err(MassaDBError::TimeError(String::from(
                            "all our changes are strictly after last_change_id, we can't be sure we did not miss any",
                        )));
                    }

                    match cursor.key() {
                        Some(cursor_change_id) => {
                            // We have to send all the updates since cursor_change_id
                            // TODO_PR: check if / how we want to limit the number of updates we send. It may be needed but tricky to implement.
                            let mut updates: BTreeMap<Vec<u8>, Option<Vec<u8>>> = BTreeMap::new();
                            let iter = self
                                .change_history
                                .range((Bound::Included(cursor_change_id), Bound::Unbounded));
                            for (_change_id, changes) in iter {
                                updates.extend(
                                    changes
                                        .range((
                                            Bound::<Vec<u8>>::Unbounded,
                                            Included(max_key.clone()),
                                        ))
                                        .map(|(k, v)| (k.clone(), v.clone())),
                                );
                            }
                            (updates, Some(max_key))
                        }
                        None => (BTreeMap::new(), Some(max_key)), // no new updates
                    }
                }
            }
        } else {
            (BTreeMap::new(), None) // we start from the beginning, so no updates on previous elements
        };

        let mut new_elements = BTreeMap::new();
        let handle = self.db.cf_handle(VERSIONING_CF).expect(CF_ERROR);

        // Creates an iterator from the next element after the last if defined, otherwise initialize it at the first key.
        let db_iterator = match max_key {
            None => self.db.iterator_cf(handle, IteratorMode::Start),
            Some(max_key) => {
                let mut iter = self
                    .db
                    .iterator_cf(handle, IteratorMode::From(&max_key, Direction::Forward));
                iter.next();
                iter
            }
        };

        for (serialized_key, serialized_value) in db_iterator.flatten() {
            if new_elements.len() < self.config.max_new_elements {
                new_elements.insert(serialized_key.to_vec(), serialized_value.to_vec());
            } else {
                break;
            }
        }

        Ok(StreamBatch {
            new_elements,
            updates_on_previous_elements,
            change_id: self.get_change_id().expect(CHANGE_ID_DESER_ERROR),
        })
    }

    /// Used for:
    /// - Bootstrap clients, to write on disk a new received Stream (reset_history: true)
    /// - Normal operations, to write changes associated to a given change_id (reset_history: false)
    ///
    pub fn write_changes(
        &mut self,
        changes: BTreeMap<Key, Option<Value>>,
        versioning_changes: BTreeMap<Key, Option<Value>>,
        change_id: Option<ChangeID>,
        reset_history: bool,
    ) -> Result<(), MassaDBError> {
        if let Some(change_id) = change_id.clone() {
            if change_id < self.get_change_id().expect(CHANGE_ID_DESER_ERROR) {
                return Err(MassaDBError::InvalidChangeID(String::from(
                    "change_id should monotonically increase after every write",
                )));
            }
        }

        let handle_state = self.db.cf_handle(STATE_CF).expect(CF_ERROR);
        let handle_metadata = self.db.cf_handle(METADATA_CF).expect(CF_ERROR);
        let handle_versioning = self.db.cf_handle(VERSIONING_CF).expect(CF_ERROR);

        *self.current_batch.lock() = WriteBatch::default();

        for (key, value) in changes.iter() {
            if let Some(value) = value {
                self.current_batch.lock().put_cf(handle_state, key, value);
                let key_hash = Hash::compute_from(key);
                let value_hash = Hash::compute_from(value);

                self.lsmtree
                    .update(
                        key_hash.to_bytes(),
                        Bytes::from(value_hash.to_bytes().to_vec()),
                    )
                    .expect(LSMTREE_ERROR);
            } else {
                self.current_batch.lock().delete_cf(handle_state, key);
                let key_hash = Hash::compute_from(key);

                self.lsmtree
                    .remove(key_hash.to_bytes())
                    .expect(LSMTREE_ERROR);
            }
        }

        // in versioning_changes, we have the data that we do not want to include in hash
        // e.g everything that is not in 'Active' state (so hashes remain compatibles)
        for (key, value) in versioning_changes.iter() {
            if let Some(value) = value {
                self.current_batch
                    .lock()
                    .put_cf(handle_versioning, key, value);
            } else {
                self.current_batch.lock().delete_cf(handle_versioning, key);
            }
        }

        if let Some(change_id) = change_id {
            self.set_change_id_to_batch(change_id);
        }

        self.current_batch
            .lock()
            .put_cf(handle_metadata, STATE_HASH_KEY, self.lsmtree.root());

        {
            let mut current_batch_guard = self.current_batch.lock();
            let batch = WriteBatch::from_data(current_batch_guard.data());
            current_batch_guard.clear();

            self.db.write(batch).map_err(|e| {
                MassaDBError::RocksDBError(format!("Can't write batch to disk: {}", e))
            })?;
        }

        self.current_hashmap.write().clear();

        self.change_history
            .entry(self.get_change_id().expect(CHANGE_ID_DESER_ERROR))
            .and_modify(|map| map.extend(changes.clone().into_iter()))
            .or_insert(changes);

        self.change_history_versioning
            .entry(self.get_change_id().expect(CHANGE_ID_DESER_ERROR))
            .and_modify(|map| map.extend(versioning_changes.clone().into_iter()))
            .or_insert(versioning_changes);

        if reset_history {
            self.change_history.clear();
        }

        while self.change_history.len() > self.config.max_history_length {
            self.change_history.pop_first();
        }

        Ok(())
    }

    /// Get the current change_id attached to the database.
    pub fn get_change_id(&self) -> Result<ChangeID, ModelsError> {
        let db = &self.db;
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);

        let Ok(Some(change_id_bytes)) = db.get_pinned_cf(handle, CHANGE_ID_KEY) else {
            return Err(ModelsError::BufferError(String::from("Could not recover change_id in database")));
        };

        let (_rest, change_id) = self
            .change_id_deserializer
            .deserialize::<DeserializeError>(&change_id_bytes)
            .expect(CHANGE_ID_DESER_ERROR);

        Ok(change_id)
    }

    /// Set the initial change_id. This function should only be called at startup/reset, as it does not batch this set with other changes.
    pub fn set_initial_change_id(&self, change_id: ChangeID) {
        self.current_batch.lock().clear();

        self.set_change_id_to_batch(change_id);

        let batch;
        {
            let mut current_batch_guard = self.current_batch.lock();
            batch = WriteBatch::from_data(current_batch_guard.data());
            current_batch_guard.clear();

            self.db.write(batch).expect(CRUD_ERROR);
        }
    }

    /// Set the current change_id in the batch
    pub fn set_change_id_to_batch(&self, change_id: ChangeID) {
        let handle_metadata = self.db.cf_handle(METADATA_CF).expect(CF_ERROR);

        let mut change_id_bytes = Vec::new();
        self.change_id_serializer
            .serialize(&change_id, &mut change_id_bytes)
            .expect(CHANGE_ID_SER_ERROR);

        self.current_batch
            .lock()
            .put_cf(handle_metadata, CHANGE_ID_KEY, &change_id_bytes);
    }

    /// Write a stream_batch of database entries received from a bootstrap server
    pub fn write_batch_bootstrap_client(
        &mut self,
        stream_changes: StreamBatch<ChangeID>,
        stream_changes_versioning: StreamBatch<ChangeID>,
    ) -> Result<(StreamingStep<Key>, StreamingStep<Key>), MassaDBError> {
        let mut changes = BTreeMap::new();

        let new_cursor = match stream_changes.new_elements.last_key_value() {
            Some((k, _)) => StreamingStep::Ongoing(k.clone()),
            None => {
                let handle = self.db.cf_handle(STATE_CF).expect(CF_ERROR);
                match self.db.iterator_cf(handle, IteratorMode::End).next() {
                    Some(Ok((serialized_key, _value))) => {
                        StreamingStep::Finished(Some(serialized_key.to_vec()))
                    }
                    _ => StreamingStep::Finished(None),
                }
            }
        };

        changes.extend(stream_changes.updates_on_previous_elements);
        changes.extend(
            stream_changes
                .new_elements
                .iter()
                .map(|(k, v)| (k.clone(), Some(v.clone()))),
        );

        let mut versioning_changes = BTreeMap::new();

        let new_cursor_versioning = match stream_changes_versioning.new_elements.last_key_value() {
            Some((k, _)) => StreamingStep::Ongoing(k.clone()),
            None => {
                let handle = self.db.cf_handle(VERSIONING_CF).expect(CF_ERROR);
                match self.db.iterator_cf(handle, IteratorMode::End).next() {
                    Some(Ok((serialized_key, _value))) => {
                        StreamingStep::Finished(Some(serialized_key.to_vec()))
                    }
                    _ => StreamingStep::Finished(None),
                }
            }
        };

        versioning_changes.extend(stream_changes_versioning.updates_on_previous_elements);
        versioning_changes.extend(
            stream_changes
                .new_elements
                .iter()
                .map(|(k, v)| (k.clone(), Some(v.clone()))),
        );

        self.write_changes(
            changes,
            versioning_changes,
            Some(stream_changes.change_id),
            true,
        )?;

        Ok((new_cursor, new_cursor_versioning))
    }

    /// Get the current state hash of the database
    fn get_db_hash_opt(&self) -> Option<Hash> {
        let db = &self.db;
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);

        db.get_cf(handle, STATE_HASH_KEY)
            .expect(CRUD_ERROR)
            .as_deref()
            .map(|state_hash_bytes| {
                Hash::from_bytes(state_hash_bytes.try_into().expect(STATE_HASH_ERROR))
            })
    }
}

impl RawMassaDB<Slot, SlotSerializer, SlotDeserializer> {
    /// Returns a new `MassaDB` instance
    pub fn new(config: MassaDBConfig) -> Self {
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        let db = DB::open_cf_descriptors(
            &db_opts,
            &config.path,
            vec![
                ColumnFamilyDescriptor::new(STATE_CF, Options::default()),
                ColumnFamilyDescriptor::new(METADATA_CF, Options::default()),
                ColumnFamilyDescriptor::new(LSMTREE_NODES_CF, Options::default()),
                ColumnFamilyDescriptor::new(LSMTREE_VALUES_CF, Options::default()),
                ColumnFamilyDescriptor::new(VERSIONING_CF, Options::default()),
            ],
        )
        .expect(OPEN_ERROR);

        let db = Arc::new(db);
        let current_batch = Arc::new(Mutex::new(WriteBatch::default()));
        let current_hashmap = Arc::new(RwLock::new(HashMap::new()));

        let change_id_deserializer = SlotDeserializer::new(
            (Included(u64::MIN), Included(u64::MAX)),
            (Included(0), Excluded(config.thread_count)),
        );

        let nodes_store = MassaDbLsmtree::new(
            LSMTREE_NODES_CF,
            db.clone(),
            current_batch.clone(),
            current_hashmap.clone(),
        );
        let values_store = MassaDbLsmtree::new(
            LSMTREE_VALUES_CF,
            db.clone(),
            current_batch.clone(),
            current_hashmap.clone(),
        );

        let handle_metadata = db.cf_handle(METADATA_CF).expect(CF_ERROR);
        let lsmtree = match db
            .get_cf(handle_metadata, STATE_HASH_KEY)
            .expect(CRUD_ERROR)
        {
            Some(hash_bytes) => SparseMerkleTree::import(nodes_store, values_store, hash_bytes),
            _ => SparseMerkleTree::new_with_stores(nodes_store, values_store),
        };

        let massa_db = Self {
            db,
            config,
            change_history: BTreeMap::new(),
            change_history_versioning: BTreeMap::new(),
            change_id_serializer: SlotSerializer::new(),
            change_id_deserializer,
            lsmtree,
            current_batch,
            current_hashmap,
        };

        if massa_db.get_change_id().is_err() {
            massa_db.set_initial_change_id(Slot {
                period: 0,
                thread: 0,
            });
        }

        massa_db
    }
}

impl MassaDBController for RawMassaDB<Slot, SlotSerializer, SlotDeserializer> {
    /// Creates a new hard copy of the DB, for the given slot
    fn backup_db(&self, slot: Slot) {
        let db = &self.db;

        let subpath = format!("backup_{}_{}", slot.period, slot.thread);

        Checkpoint::new(db)
            .expect("Cannot init checkpoint")
            .create_checkpoint(db.path().join(subpath))
            .expect("Failed to create checkpoint");
    }

    /// Writes the batch to the DB
    fn write_batch(&mut self, batch: DBBatch, versioning_batch: DBBatch, change_id: Option<Slot>) {
        self.write_changes(batch, versioning_batch, change_id, false)
            .expect(CRUD_ERROR);
    }

    /// Utility function to put / update a key & value in the batch
    fn put_or_update_entry_value(&self, batch: &mut DBBatch, key: Vec<u8>, value: &[u8]) {
        batch.insert(key, Some(value.to_vec()));
    }

    /// Utility function to delete a key & value in the batch
    fn delete_key(&self, batch: &mut DBBatch, key: Vec<u8>) {
        batch.insert(key, None);
    }

    /// Utility function to delete all keys in a prefix
    fn delete_prefix(&mut self, prefix: &str, handle_str: &str, change_id: Option<Slot>) {
        let db = &self.db;

        let handle = db.cf_handle(handle_str).expect(CF_ERROR);
        let mut batch = DBBatch::new();
        for (serialized_key, _) in db.prefix_iterator_cf(handle, prefix).flatten() {
            if !serialized_key.starts_with(prefix.as_bytes()) {
                break;
            }

            self.delete_key(&mut batch, serialized_key.to_vec());
        }

        match handle_str {
            STATE_CF => {
                self.write_batch(batch, DBBatch::new(), change_id);
            }
            VERSIONING_CF => {
                self.write_batch(DBBatch::new(), batch, change_id);
            }
            _ => {}
        }
    }

    /// Reset the database, and attach it to the given slot.
    fn reset(&mut self, slot: Slot) {
        self.set_initial_change_id(slot);
        self.change_history.clear();
        self.current_hashmap.write().clear();
    }

    fn get_cf(&self, handle_cf: &str, key: Key) -> Result<Option<Value>, MassaDBError> {
        let db = &self.db;
        let handle = db.cf_handle(handle_cf).expect(CF_ERROR);

        db.get_cf(handle, key)
            .map_err(|e| MassaDBError::RocksDBError(format!("{:?}", e)))
    }

    /// Exposes RocksDB's "multi_get_cf" function
    fn multi_get_cf(&self, query: Vec<(&str, Key)>) -> Vec<Result<Option<Value>, MassaDBError>> {
        let db = &self.db;

        let rocks_db_query = query
            .into_iter()
            .map(|(handle_cf, key)| (db.cf_handle(handle_cf).expect(CF_ERROR), key))
            .collect::<Vec<_>>();

        db.multi_get_cf(rocks_db_query)
            .into_iter()
            .map(|res| res.map_err(|e| MassaDBError::RocksDBError(format!("{:?}", e))))
            .collect()
    }

    /// Exposes RocksDB's "iterator_cf" function
    fn iterator_cf(
        &self,
        handle_cf: &str,
        mode: MassaIteratorMode,
    ) -> Box<dyn Iterator<Item = (Key, Value)> + '_> {
        let db = &self.db;
        let handle = db.cf_handle(handle_cf).expect(CF_ERROR);

        let rocksdb_mode = match mode {
            MassaIteratorMode::Start => IteratorMode::Start,
            MassaIteratorMode::End => IteratorMode::End,
            MassaIteratorMode::From(key, MassaDirection::Forward) => {
                IteratorMode::From(key, Direction::Forward)
            }
            MassaIteratorMode::From(key, MassaDirection::Reverse) => {
                IteratorMode::From(key, Direction::Reverse)
            }
        };

        Box::new(
            db.iterator_cf(handle, rocksdb_mode)
                .flatten()
                .map(|(k, v)| (k.to_vec(), v.to_vec())),
        )
    }

    /// Exposes RocksDB's "prefix_iterator_cf" function
    fn prefix_iterator_cf(
        &self,
        handle_cf: &str,
        prefix: &[u8],
    ) -> Box<dyn Iterator<Item = (Key, Value)> + '_> {
        let db = &self.db;
        let handle = db.cf_handle(handle_cf).expect(CF_ERROR);

        Box::new(
            db.prefix_iterator_cf(handle, prefix)
                .flatten()
                .map(|(k, v)| (k.to_vec(), v.to_vec())),
        )
    }

    /// Get the current state hash of the database
    fn get_db_hash(&self) -> Hash {
        self.get_db_hash_opt()
            .unwrap_or(Hash::from_bytes(STATE_HASH_INITIAL_BYTES))
    }

    /// Get the current change_id attached to the database.
    fn get_change_id(&self) -> Result<Slot, ModelsError> {
        self.get_change_id()
    }

    /// Set the initial change_id. This function should only be called at startup/reset, as it does not batch this set with other changes.
    fn set_initial_change_id(&self, change_id: Slot) {
        self.set_initial_change_id(change_id)
    }

    /// Flushes the underlying db.
    fn flush(&self) -> Result<(), MassaDBError> {
        self.db
            .flush()
            .map_err(|e| MassaDBError::RocksDBError(format!("{:?}", e)))
    }

    /// Write a stream_batch of database entries received from a bootstrap server
    fn write_batch_bootstrap_client(
        &mut self,
        stream_changes: StreamBatch<Slot>,
        stream_changes_versioning: StreamBatch<Slot>,
    ) -> Result<(StreamingStep<Key>, StreamingStep<Key>), MassaDBError> {
        self.write_batch_bootstrap_client(stream_changes, stream_changes_versioning)
    }

    /// Used for bootstrap servers (get a new batch of data from STATE_CF to stream to the client)
    ///
    /// Returns a StreamBatch<Slot>
    fn get_batch_to_stream(
        &self,
        last_obtained: Option<(Vec<u8>, Slot)>,
    ) -> Result<StreamBatch<Slot>, MassaDBError> {
        self.get_batch_to_stream(last_obtained)
    }

    /// Used for bootstrap servers (get a new batch of data from VERSIONING_CF to stream to the client)
    ///
    /// Returns a StreamBatch<Slot>
    fn get_versioning_batch_to_stream(
        &self,
        last_obtained: Option<(Vec<u8>, Slot)>,
    ) -> Result<StreamBatch<Slot>, MassaDBError> {
        self.get_versioning_batch_to_stream(last_obtained)
    }
}
