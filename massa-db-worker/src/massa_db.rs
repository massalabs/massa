use massa_db_exports::{
    DBBatch, Key, MassaDBConfig, MassaDBController, MassaDBError, MassaDirection,
    MassaIteratorMode, StreamBatch, Value, CF_ERROR, CHANGE_ID_DESER_ERROR, CHANGE_ID_KEY,
    CHANGE_ID_SER_ERROR, CRUD_ERROR, METADATA_CF, OPEN_ERROR, STATE_CF, STATE_HASH_ERROR,
    STATE_HASH_INITIAL_BYTES, STATE_HASH_KEY, VERSIONING_CF,
};
use massa_hash::{HashXof, HASH_XOF_SIZE_BYTES};
use massa_models::{
    error::ModelsError,
    slot::{Slot, SlotDeserializer, SlotSerializer},
    streaming_step::StreamingStep,
};
use massa_serialization::{DeserializeError, Deserializer, Serializer, U64VarIntSerializer};
use parking_lot::Mutex;
use rocksdb::{
    checkpoint::Checkpoint, ColumnFamilyDescriptor, Direction, IteratorMode, Options, WriteBatch,
    DB,
};
use std::path::PathBuf;
use std::{
    collections::BTreeMap,
    format,
    ops::Bound::{self, Excluded, Included, Unbounded},
    sync::Arc,
};

/// Wrapped RocksDB database
///
/// In our instance, we use Slot as the ChangeID
pub type MassaDB = RawMassaDB<Slot, SlotSerializer, SlotDeserializer>;

/// A generic wrapped RocksDB database.
///
/// The added features are:
/// - Hash tracking with Xor
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
    pub config: MassaDBConfig,
    /// In change_history, we keep the latest changes made to the database, useful for streaming them to a client.
    pub change_history: BTreeMap<ChangeID, BTreeMap<Key, Option<Value>>>,
    /// same as change_history but for versioning
    pub change_history_versioning: BTreeMap<ChangeID, BTreeMap<Key, Option<Value>>>,
    /// A serializer for the ChangeID type
    pub change_id_serializer: ChangeIDSerializer,
    /// A deserializer for the ChangeID type
    pub change_id_deserializer: ChangeIDDeserializer,
    /// The current RocksDB batch of the database, in a Mutex to share it
    pub current_batch: Arc<Mutex<WriteBatch>>,
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
        last_state_step: &StreamingStep<Vec<u8>>,
        last_change_id: Option<ChangeID>,
    ) -> Result<StreamBatch<ChangeID>, MassaDBError> {
        let bound_key_for_changes = match &last_state_step {
            StreamingStep::Ongoing(max_key) => Included(max_key.clone()),
            _ => Unbounded,
        };

        // Updates == "everything that changed since the last change_id streamed, up to a certain key".
        // This definition also applies to keys that were not in the DB beforehand.
        let updates_on_previous_elements = match (&last_state_step, last_change_id) {
            (StreamingStep::Started, _) => {
                // Stream No changes, new elements from start
                BTreeMap::new()
            }
            (_, Some(last_change_id)) => {
                // Stream the changes depending on the previously computed bound

                match last_change_id.cmp(&self.get_change_id().expect(CHANGE_ID_DESER_ERROR)) {
                    std::cmp::Ordering::Greater => {
                        return Err(MassaDBError::CacheMissError(String::from(
                            "we don't have this change yet on this node (it's in the future for us)",
                        )));
                    }
                    std::cmp::Ordering::Equal => {
                        BTreeMap::new() // no new updates
                    }
                    std::cmp::Ordering::Less => {
                        // We should send all the new updates since last_change_id
                        // We check that last_change_id is in our history
                        match self.change_history.keys().next() {
                            Some(first_change_id_in_history)
                                if first_change_id_in_history <= &last_change_id =>
                            {
                                let mut updates: BTreeMap<Vec<u8>, Option<Vec<u8>>> =
                                    BTreeMap::new();
                                // TODO: check if / how we want to limit the number of updates we send. It may be needed but tricky to implement.
                                // NOTE: We send again the changes associated to last_change_id (Bound::Included),
                                // in case multiple writes happened in the same slot.
                                // This should not happen in the current codebase, but it may be done in the future.
                                let iter = self
                                    .change_history
                                    .range((Bound::Included(last_change_id), Bound::Unbounded));
                                for (_change_id, changes) in iter {
                                    updates.extend(
                                        changes
                                            .range((
                                                Bound::<Vec<u8>>::Unbounded,
                                                bound_key_for_changes.clone(),
                                            ))
                                            .map(|(k, v)| (k.clone(), v.clone())),
                                    );
                                }
                                updates
                            }
                            _ => {
                                // Here, we have been asked for the changes associated to a change_id that is not in our history.
                                // More than `max_history_length` slots have happen since last_change_id, and we have deleted the changes associated to this change_id.
                                // This can happen if the client has a poor connection, or if the network is unstable.
                                return Err(MassaDBError::CacheMissError(String::from(
                                    "all our changes are strictly after last_change_id, we can't be sure we did not miss any",
                                )));
                            }
                        }
                    }
                }
            }
            _ => {
                // last_change_id is None, but StreamingStep is either Ongoing or Finished
                return Err(MassaDBError::TimeError(String::from(
                    "State streaming was ongoing or finished, but no last_change_id was provided",
                )));
            }
        };

        let mut new_elements = BTreeMap::new();
        let mut new_elements_size = 0;

        if !last_state_step.finished() {
            let handle = self.db.cf_handle(STATE_CF).expect(CF_ERROR);

            // Creates an iterator from the next element after the last if defined, otherwise initialize it at the first key.
            let db_iterator = match &last_state_step {
                StreamingStep::Ongoing(max_key) => {
                    let mut iter = self
                        .db
                        .iterator_cf(handle, IteratorMode::From(max_key, Direction::Forward));
                    iter.next();
                    iter
                }
                _ => self.db.iterator_cf(handle, IteratorMode::Start),
            };

            let u64_ser = U64VarIntSerializer::new();
            for (serialized_key, serialized_value) in db_iterator.flatten() {
                let key_len = serialized_key.len();
                let value_len = serialized_value.len();
                let mut buffer = Vec::new();
                u64_ser
                    .serialize(&(key_len as u64), &mut buffer)
                    .map_err(|_| {
                        MassaDBError::SerializeError(String::from("Cannot serialize key length"))
                    })?;
                u64_ser
                    .serialize(&(value_len as u64), &mut buffer)
                    .map_err(|_| {
                        MassaDBError::SerializeError(String::from("Cannot serialize value length"))
                    })?;
                // We consider the total byte size of the serialized elements (with VecU8Serializer) to fill the StreamBatch,
                // in order to make deserialization easier
                new_elements_size += key_len + value_len + buffer.len();
                if new_elements_size <= self.config.max_final_state_elements_size {
                    new_elements.insert(serialized_key.to_vec(), serialized_value.to_vec());
                } else {
                    break;
                }
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
        last_versioning_step: &StreamingStep<Vec<u8>>,
        last_change_id: Option<ChangeID>,
    ) -> Result<StreamBatch<ChangeID>, MassaDBError> {
        let bound_key_for_changes = match &last_versioning_step {
            StreamingStep::Ongoing(max_key) => Included(max_key.clone()),
            _ => Unbounded,
        };

        let updates_on_previous_elements = match (&last_versioning_step, last_change_id) {
            (StreamingStep::Started, _) => {
                // Stream No changes, new elements from start
                BTreeMap::new()
            }
            (_, Some(last_change_id)) => {
                // Stream the changes depending on the previously computed bound

                match last_change_id.cmp(&self.get_change_id().expect(CHANGE_ID_DESER_ERROR)) {
                    std::cmp::Ordering::Greater => {
                        return Err(MassaDBError::CacheMissError(String::from(
                            "we don't have this change yet on this node (it's in the future for us)",
                        )));
                    }
                    std::cmp::Ordering::Equal => {
                        BTreeMap::new() // no new updates
                    }
                    std::cmp::Ordering::Less => {
                        // We should send all the new updates since last_change_id
                        // We check that last_change_id is in our history
                        match self.change_history_versioning.keys().next() {
                            Some(first_change_id_in_history)
                                if first_change_id_in_history <= &last_change_id =>
                            {
                                let mut updates: BTreeMap<Vec<u8>, Option<Vec<u8>>> =
                                    BTreeMap::new();
                                // TODO: check if / how we want to limit the number of updates we send. It may be needed but tricky to implement.
                                // NOTE: We send again the changes associated to last_change_id (Bound::Included),
                                // in case multiple writes happened in the same slot.
                                // This should not happen in the current codebase, but it may be done in the future.
                                let iter = self
                                    .change_history_versioning
                                    .range((Bound::Included(last_change_id), Bound::Unbounded));
                                for (_change_id, changes) in iter {
                                    updates.extend(
                                        changes
                                            .range((
                                                Bound::<Vec<u8>>::Unbounded,
                                                bound_key_for_changes.clone(),
                                            ))
                                            .map(|(k, v)| (k.clone(), v.clone())),
                                    );
                                }
                                updates
                            }
                            _ => {
                                // Here, we have been asked for the changes associated to a change_id that is not in our history.
                                // More than `max_history_length` slots have happen since last_change_id, and we have deleted the changes associated to this change_id.
                                // This can happen if the client has a poor connection, or if the network is unstable.
                                return Err(MassaDBError::CacheMissError(String::from(
                                    "all our changes are strictly after last_change_id, we can't be sure we did not miss any",
                                )));
                            }
                        }
                    }
                }
            }
            _ => {
                // last_change_id is None, but StreamingStep is either Ongoing or Finished
                return Err(MassaDBError::TimeError(String::from(
                    "State streaming was ongoing or finished, but no last_change_id was provided",
                )));
            }
        };

        let mut new_elements = BTreeMap::new();
        let mut new_elements_size = 0;

        if !last_versioning_step.finished() {
            let handle = self.db.cf_handle(VERSIONING_CF).expect(CF_ERROR);

            // Creates an iterator from the next element after the last if defined, otherwise initialize it at the first key.
            let db_iterator = match &last_versioning_step {
                StreamingStep::Ongoing(max_key) => {
                    let mut iter = self
                        .db
                        .iterator_cf(handle, IteratorMode::From(max_key, Direction::Forward));
                    iter.next();
                    iter
                }
                _ => self.db.iterator_cf(handle, IteratorMode::Start),
            };
            let u64_ser = U64VarIntSerializer::new();
            for (serialized_key, serialized_value) in db_iterator.flatten() {
                let key_len = serialized_key.len();
                let value_len = serialized_value.len();
                let mut buffer = Vec::new();
                u64_ser
                    .serialize(&(key_len as u64), &mut buffer)
                    .map_err(|_| {
                        MassaDBError::SerializeError(String::from("Cannot serialize key length"))
                    })?;
                u64_ser
                    .serialize(&(value_len as u64), &mut buffer)
                    .map_err(|_| {
                        MassaDBError::SerializeError(String::from("Cannot serialize value length"))
                    })?;
                // We consider the total byte size of the serialized elements (with VecU8Serializer) to fill the StreamBatch,
                // in order to make deserialization easier
                new_elements_size += key_len + value_len + buffer.len();
                if new_elements_size <= self.config.max_versioning_elements_size {
                    new_elements.insert(serialized_key.to_vec(), serialized_value.to_vec());
                } else {
                    break;
                }
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

        let mut current_xor_hash = self.get_xof_db_hash();

        *self.current_batch.lock() = WriteBatch::default();

        for (key, value) in changes.iter() {
            if let Some(value) = value {
                self.current_batch.lock().put_cf(handle_state, key, value);

                // Compute the XOR in all cases
                if let Ok(Some(prev_value)) = self.db.get_cf(handle_state, key) {
                    let prev_hash =
                        HashXof::compute_from_tuple(&[key.as_slice(), prev_value.as_slice()]);
                    current_xor_hash ^= prev_hash;
                };
                let new_hash = HashXof::compute_from_tuple(&[key.as_slice(), value.as_slice()]);
                current_xor_hash ^= new_hash;
            } else {
                self.current_batch.lock().delete_cf(handle_state, key);

                // Compute the XOR in all cases
                if let Ok(Some(prev_value)) = self.db.get_cf(handle_state, key) {
                    let prev_hash =
                        HashXof::compute_from_tuple(&[key.as_slice(), prev_value.as_slice()]);
                    current_xor_hash ^= prev_hash;
                };
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

        // Update the hash entry
        self.current_batch
            .lock()
            .put_cf(handle_metadata, STATE_HASH_KEY, current_xor_hash.0);

        {
            let mut current_batch_guard = self.current_batch.lock();
            let batch = WriteBatch::from_data(current_batch_guard.data());
            current_batch_guard.clear();

            self.db.write(batch).map_err(|e| {
                MassaDBError::RocksDBError(format!("Can't write batch to disk: {}", e))
            })?;
        }

        match self
            .change_history
            .entry(self.get_change_id().expect(CHANGE_ID_DESER_ERROR))
        {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(changes);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().extend(changes);
            }
        }

        match self
            .change_history_versioning
            .entry(self.get_change_id().expect(CHANGE_ID_DESER_ERROR))
        {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(versioning_changes);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().extend(versioning_changes);
            }
        }

        if reset_history {
            self.change_history.clear();
            self.change_history_versioning.clear();
        }

        while self.change_history.len() > self.config.max_history_length {
            self.change_history.pop_first();
        }

        while self.change_history_versioning.len() > self.config.max_history_length {
            self.change_history_versioning.pop_first();
        }

        Ok(())
    }

    /// Get the current change_id attached to the database.
    pub fn get_change_id(&self) -> Result<ChangeID, ModelsError> {
        let db = &self.db;
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);

        let Ok(Some(change_id_bytes)) = db.get_pinned_cf(handle, CHANGE_ID_KEY) else {
            return Err(ModelsError::BufferError(String::from(
                "Could not recover change_id in database",
            )));
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

        let new_cursor: StreamingStep<Vec<u8>> = match stream_changes.new_elements.last_key_value()
        {
            Some((k, _)) => StreamingStep::Ongoing(k.clone()),
            None => StreamingStep::Finished(None),
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
            None => StreamingStep::Finished(None),
        };

        versioning_changes.extend(stream_changes_versioning.updates_on_previous_elements);
        versioning_changes.extend(
            stream_changes_versioning
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

    /// Get the current XOF state hash of the database
    pub fn get_xof_db_hash(&self) -> HashXof<HASH_XOF_SIZE_BYTES> {
        self.get_xof_db_hash_opt()
            .unwrap_or(HashXof(*STATE_HASH_INITIAL_BYTES))
    }

    /// Get the current XOF state hash of the database
    fn get_xof_db_hash_opt(&self) -> Option<HashXof<HASH_XOF_SIZE_BYTES>> {
        let db = &self.db;
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);

        db.get_cf(handle, STATE_HASH_KEY)
            .expect(CRUD_ERROR)
            .as_deref()
            .map(|state_hash_bytes| HashXof(state_hash_bytes.try_into().expect(STATE_HASH_ERROR)))
    }
}

impl RawMassaDB<Slot, SlotSerializer, SlotDeserializer> {
    /// Returns a new `MassaDB` instance
    pub fn new(config: MassaDBConfig) -> Self {
        let db_opts = Self::default_db_opts();
        Self::new_with_options(config, db_opts).expect(OPEN_ERROR)
    }

    pub fn default_db_opts() -> Options {
        let mut db_opts = Options::default();
        db_opts.set_max_open_files(820);
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);
        db_opts
    }

    /// Returns a new `MassaDB` instance given a config and RocksDB options
    fn new_with_options(config: MassaDBConfig, db_opts: Options) -> Result<Self, rocksdb::Error> {
        let db = DB::open_cf_descriptors(
            &db_opts,
            &config.path,
            vec![
                ColumnFamilyDescriptor::new(STATE_CF, Options::default()),
                ColumnFamilyDescriptor::new(METADATA_CF, Options::default()),
                ColumnFamilyDescriptor::new(VERSIONING_CF, Options::default()),
            ],
        )?;

        let db = Arc::new(db);
        let current_batch = Arc::new(Mutex::new(WriteBatch::default()));

        let change_id_deserializer = SlotDeserializer::new(
            (Included(u64::MIN), Included(u64::MAX)),
            (Included(0), Excluded(config.thread_count)),
        );

        let massa_db = Self {
            db,
            config,
            change_history: BTreeMap::new(),
            change_history_versioning: BTreeMap::new(),
            change_id_serializer: SlotSerializer::new(),
            change_id_deserializer,
            current_batch,
        };

        if massa_db.get_change_id().is_err() {
            massa_db.set_initial_change_id(Slot {
                period: 0,
                thread: 0,
            });
        }

        Ok(massa_db)
    }
}

impl MassaDBController for RawMassaDB<Slot, SlotSerializer, SlotDeserializer> {
    /// Creates a new hard copy of the DB, for the given slot
    fn backup_db(&self, slot: Slot) -> PathBuf {
        let db = &self.db;
        let subpath = format!("backup_{}_{}", slot.period, slot.thread);

        let previous_backups_paths = std::fs::read_dir(db.path())
            .expect("Cannot walk db path")
            .map(|res| res.map(|e| e.path()))
            .collect::<Result<Vec<_>, std::io::Error>>()
            .expect("Cannot walk db path");

        let mut previous_backups = BTreeMap::new();

        for backup_path in previous_backups_paths.iter() {
            let Some(path_str) = backup_path.file_name().and_then(|f| f.to_str()) else {
                continue;
            };
            let vec = path_str.split('_').collect::<Vec<&str>>();
            if vec.len() == 3 && vec[0] == "backup" {
                let Ok(period) = vec[1].parse::<u64>() else {
                    continue;
                };
                let Ok(thread) = vec[2].parse::<u8>() else {
                    continue;
                };
                let backup_slot = Slot::new(period, thread);
                previous_backups.insert(backup_slot, backup_path);
            }
        }

        // Remove the oldest backups if we have too many
        while previous_backups.len() >= self.config.max_ledger_backups as usize {
            if let Some((_, oldest_backup_path)) = previous_backups.pop_first() {
                std::fs::remove_dir_all(oldest_backup_path).expect("Cannot remove oldest backup");
            }
        }

        let backup_path = db.path().join(subpath);
        println!("backup_path: {:?}", backup_path);
        Checkpoint::new(db)
            .expect("Cannot init checkpoint")
            .create_checkpoint(backup_path.clone())
            .expect("Failed to create checkpoint");

        backup_path
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
    ///
    /// This function is used in the FinalStateController::reset method which is used in the Bootstrap
    /// process when the bootstrap fails (Bootstrap slot too old). A bootstrap to another node will likely occur
    /// after this reset.
    fn reset(&mut self, slot: Slot) {
        // For dev: please take care of correctly reset the db to avoid any issue when the bootstrap
        //          process is restarted
        self.set_initial_change_id(slot); // Note: this also reset the field: current_batch
        self.change_history.clear();
        self.change_history_versioning.clear();
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

    /// Get the current extended state hash of the database
    fn get_xof_db_hash(&self) -> HashXof<HASH_XOF_SIZE_BYTES> {
        self.get_xof_db_hash()
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
        last_state_step: &StreamingStep<Vec<u8>>,
        last_change_id: Option<Slot>,
    ) -> Result<StreamBatch<Slot>, MassaDBError> {
        self.get_batch_to_stream(last_state_step, last_change_id)
    }

    /// Used for bootstrap servers (get a new batch of data from VERSIONING_CF to stream to the client)
    ///
    /// Returns a StreamBatch<Slot>
    fn get_versioning_batch_to_stream(
        &self,
        last_versioning_step: &StreamingStep<Vec<u8>>,
        last_change_id: Option<Slot>,
    ) -> Result<StreamBatch<Slot>, MassaDBError> {
        self.get_versioning_batch_to_stream(last_versioning_step, last_change_id)
    }

    #[cfg(feature = "test-exports")]
    fn get_entire_database(&self) -> Vec<BTreeMap<Vec<u8>, Vec<u8>>> {
        let handle_state = self.db.cf_handle(STATE_CF).expect(CF_ERROR);
        let handle_metadata = self.db.cf_handle(METADATA_CF).expect(CF_ERROR);
        let handle_versioning = self.db.cf_handle(VERSIONING_CF).expect(CF_ERROR);
        let iter_state = self.db.iterator_cf(handle_state, IteratorMode::Start);
        let iter_metadata = self.db.iterator_cf(handle_metadata, IteratorMode::Start);
        let iter_versioning = self.db.iterator_cf(handle_versioning, IteratorMode::Start);
        let mut entire_database = Vec::new();
        let mut state = BTreeMap::new();
        for (k, v) in iter_state.flatten() {
            state.insert(k.to_vec(), v.to_vec());
        }
        let mut metadata = BTreeMap::new();
        for (k, v) in iter_metadata.flatten() {
            metadata.insert(k.to_vec(), v.to_vec());
        }
        let mut versioning = BTreeMap::new();
        for (k, v) in iter_versioning.flatten() {
            versioning.insert(k.to_vec(), v.to_vec());
        }
        entire_database.push(state);
        entire_database.push(metadata);
        entire_database.push(versioning);
        entire_database
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use assert_matches::assert_matches;
    use massa_db_exports::MassaDBError::TimeError;
    use parking_lot::RwLock;
    use tempfile::tempdir;

    use massa_hash::Hash;
    use massa_models::config::THREAD_COUNT;
    use massa_models::streaming_step::StreamingStep;

    use super::*;

    fn dump_column(
        db: Arc<RwLock<Box<dyn MassaDBController>>>,
        column: &str,
    ) -> BTreeMap<Vec<u8>, Vec<u8>> {
        db.read()
            .iterator_cf(column, MassaIteratorMode::Start)
            // .collect::<BTreeMap<Vec<u8>, Vec<u8>>>()
            .collect()
    }

    fn dump_column_opt(
        db: Arc<RwLock<Box<dyn MassaDBController>>>,
        column: &str,
    ) -> BTreeMap<Vec<u8>, Option<Vec<u8>>> {
        db.read()
            .iterator_cf(column, MassaIteratorMode::Start)
            .map(|(k, v)| (k, Some(v)))
            // .collect::<BTreeMap<Vec<u8>, Option<Vec<u8>>>>()
            .collect()
    }

    fn initial_hash() -> Hash {
        // Initial hash in the db / final state
        Hash::compute_from(STATE_HASH_INITIAL_BYTES)
    }

    #[test]
    fn test_init() {
        // Test we cannot double init a Db object

        let temp_dir_db = tempdir().expect("Unable to create a temp folder");
        // println!("temp_dir_db: {:?}", temp_dir_db);
        let db_config = MassaDBConfig {
            path: temp_dir_db.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };
        let mut db_opts = MassaDB::default_db_opts();
        // Additional checks (only for testing)
        db_opts.set_paranoid_checks(true);

        let _db = MassaDB::new_with_options(db_config.clone(), db_opts.clone()).unwrap();

        // Check not allowed to init a second instance
        let db2 = MassaDB::new_with_options(db_config, db_opts.clone());
        assert!(db2.is_err());
        assert!(db2.err().unwrap().into_string().contains("IO error"));
    }

    #[test]
    fn test_basics_1() {
        // 1- Init a db + check initial hash
        // 2- Add some data
        // 3- Read it
        // 4- Remove data

        // Init
        let temp_dir_db = tempdir().expect("Unable to create a temp folder");
        // println!("temp_dir_db: {:?}", temp_dir_db);
        let db_config = MassaDBConfig {
            path: temp_dir_db.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };
        let mut db_opts = MassaDB::default_db_opts();
        // Additional checks (only for testing)
        db_opts.set_paranoid_checks(true);

        let _db = MassaDB::new_with_options(db_config, db_opts.clone()).unwrap();
        let db = Arc::new(RwLock::new(
            Box::new(_db) as Box<(dyn MassaDBController + 'static)>
        ));

        assert_eq!(
            Hash::compute_from(db.read().get_xof_db_hash().to_bytes()),
            initial_hash()
        );

        // Checks up
        let cfs = rocksdb::DB::list_cf(&db_opts, temp_dir_db.path()).unwrap_or_default();
        let cf_exists_1 = cfs.iter().any(|cf| cf == "state");
        let cf_exists_2 = cfs.iter().any(|cf| cf == "metadata");
        let cf_exists_3 = cfs.iter().any(|cf| cf == "versioning");
        println!("cfs: {:?}", cfs);
        assert!(cf_exists_1);
        assert!(cf_exists_2);
        assert!(cf_exists_3);
        for col in cfs {
            if col == "metadata" {
                continue;
            }
            assert!(dump_column(db.clone(), col.as_str()).is_empty());
        }

        // Add data

        let mut batch = DBBatch::new();
        let b_key1 = vec![1, 2, 3];
        let b_value1 = vec![4, 5, 6];
        batch.insert(b_key1.clone(), Some(b_value1));
        let mut versioning_batch = BTreeMap::default();
        let vb_key1 = vec![10, 20, 30];
        let vb_value1 = vec![40, 50, 60];
        versioning_batch.insert(vb_key1.clone(), Some(vb_value1));

        let mut guard = db.write();
        guard.write_batch(batch.clone(), versioning_batch.clone(), None);
        drop(guard);

        // assert_eq!(dump_column_opt(db_.clone(), "state"), batch);
        assert_eq!(dump_column_opt(db.clone(), "state"), batch);
        // assert_eq!(dump_column_opt(db_.clone(), "versioning"), versioning_batch);
        assert_eq!(dump_column_opt(db.clone(), "versioning"), versioning_batch);

        // Remove data

        let mut batch_2 = DBBatch::new();
        batch_2.insert(b_key1, None);
        let mut versioning_batch_2 = BTreeMap::default();
        versioning_batch_2.insert(vb_key1, None);
        let mut guard = db.write();
        guard.write_batch(batch_2.clone(), versioning_batch_2.clone(), None);
        drop(guard);

        // assert!(dump_column(db_.clone(), "state").is_empty());
        assert!(dump_column(db.clone(), "state").is_empty());
        // assert!(dump_column(db_.clone(), "versioning").is_empty());
        assert!(dump_column(db.clone(), "versioning").is_empty());
    }

    #[test]
    fn test_basics_2() {
        // 1- Init a db + check initial hash
        // 2- Add some data (using  put_or_update_entry_value)
        // 3- Read it
        // 4- Remove data (using delete_prefix)

        // Init
        let temp_dir_db = tempdir().expect("Unable to create a temp folder");
        // println!("temp_dir_db: {:?}", temp_dir_db);
        let db_config = MassaDBConfig {
            path: temp_dir_db.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };
        let mut db_opts = MassaDB::default_db_opts();
        // Additional checks (only for testing)
        db_opts.set_paranoid_checks(true);

        let _db = MassaDB::new_with_options(db_config, db_opts.clone()).unwrap();
        let db = Arc::new(RwLock::new(
            Box::new(_db) as Box<(dyn MassaDBController + 'static)>
        ));

        assert_eq!(
            Hash::compute_from(db.read().get_xof_db_hash().to_bytes()),
            initial_hash()
        );

        // Add data

        let b_key1 = vec![1, 2, 3];
        let b_value1 = vec![4, 5, 6];
        let mut batch = DBBatch::new();
        db.read()
            .put_or_update_entry_value(&mut batch, b_key1.clone(), &b_value1);
        let versioning_batch = BTreeMap::default();

        let mut guard = db.write();
        guard.write_batch(batch.clone(), versioning_batch.clone(), None);
        drop(guard);

        // assert_eq!(dump_column_opt(db_.clone(), "state"), batch);
        assert_eq!(dump_column_opt(db.clone(), "state"), batch);
        // assert_eq!(dump_column_opt(db_.clone(), "versioning"), versioning_batch);
        assert_eq!(dump_column_opt(db.clone(), "versioning"), versioning_batch);

        // Remove data

        let mut batch_2 = DBBatch::new();
        // batch_2.insert(b_key1.clone(), None);
        db.read().delete_key(&mut batch_2, b_key1);
        let versioning_batch_2 = BTreeMap::default();
        let mut guard = db.write();
        guard.write_batch(batch_2.clone(), versioning_batch_2, None);
        drop(guard);

        // assert!(dump_column(db_.clone(), "state").is_empty());
        assert!(dump_column(db.clone(), "state").is_empty());
        // assert!(dump_column(db_.clone(), "versioning").is_empty());
        assert!(dump_column(db.clone(), "versioning").is_empty());

        // Add some data then remove using prefix
        batch.clear();
        db.read()
            .put_or_update_entry_value(&mut batch, vec![97, 98, 1], &[1]);
        db.read()
            .put_or_update_entry_value(&mut batch, vec![97, 98, 2], &[2]);
        let v3 = vec![200, 101, 1];
        let k3 = vec![101];
        db.read()
            .put_or_update_entry_value(&mut batch, v3.clone(), &k3);

        db.write()
            .write_batch(batch.clone(), versioning_batch, None);

        // "ab".as_bytes == [97, 98]
        db.write().delete_prefix("ab", "state", None);
        assert_eq!(dump_column(db.clone(), "state"), BTreeMap::from([(v3, k3)]))
    }

    #[test]
    fn test_backup() {
        // 1- Init a db + add data
        // 2- Backup (slot 1)
        // 3- Add more data + backup (slot 2)
        // 4- Init from backup 1 + checks
        // 5- Init from backup 2 + checks

        // Init
        let temp_dir_db = tempdir().expect("Unable to create a temp folder");
        // println!("temp_dir_db: {:?}", temp_dir_db);
        let db_config = MassaDBConfig {
            path: temp_dir_db.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };
        let mut db_opts = MassaDB::default_db_opts();
        // Additional checks (only for testing)
        db_opts.set_paranoid_checks(true);

        let _db = MassaDB::new_with_options(db_config, db_opts.clone()).unwrap();
        let db = Arc::new(RwLock::new(
            Box::new(_db) as Box<(dyn MassaDBController + 'static)>
        ));

        // Add data
        let batch = DBBatch::from([(vec![1, 2, 3], Some(vec![4, 5, 6]))]);
        let versioning_batch = DBBatch::from([(vec![10, 20, 30], Some(vec![127, 128, 254, 255]))]);
        let slot_1 = Slot::new(1, 0);
        let mut guard = db.write();
        guard.write_batch(batch, versioning_batch, Some(slot_1));
        drop(guard);

        // Keep hash so we can compare with db backup
        let hash_1 = db.read().get_xof_db_hash();

        // Backup db
        let guard = db.read();
        let backup_1 = guard.backup_db(slot_1);
        drop(guard);

        // Add data
        let batch = DBBatch::from([(vec![11, 22, 33], Some(vec![44, 55, 66]))]);
        let versioning_batch = DBBatch::from([(vec![12, 23, 34], Some(vec![255, 254, 253]))]);
        let slot_2 = Slot::new(2, 0);
        let mut guard = db.write();
        guard.write_batch(batch, versioning_batch, Some(slot_2));
        drop(guard);

        let hash_2 = db.read().get_xof_db_hash();

        // Backup db (again)
        let guard = db.read();
        let backup_2 = guard.backup_db(slot_2);
        drop(guard);

        {
            let db_backup_1_config = MassaDBConfig {
                path: backup_1,
                max_history_length: 100,
                max_final_state_elements_size: 100,
                max_versioning_elements_size: 100,
                thread_count: THREAD_COUNT,
                max_ledger_backups: 10,
            };
            let mut db_backup_1_opts = MassaDB::default_db_opts();
            db_backup_1_opts.create_if_missing(false);
            let db_backup_1 = Arc::new(RwLock::new(Box::new(
                MassaDB::new_with_options(db_backup_1_config, db_backup_1_opts.clone()).unwrap(),
            )
                as Box<(dyn MassaDBController + 'static)>));

            assert_eq!(db_backup_1.read().get_xof_db_hash(), hash_1);
            assert_ne!(db_backup_1.read().get_xof_db_hash(), hash_2);
            assert_ne!(
                Hash::compute_from(db_backup_1.read().get_xof_db_hash().to_bytes()),
                initial_hash()
            );
        }

        {
            let db_backup_2_config = MassaDBConfig {
                path: backup_2,
                max_history_length: 100,
                max_final_state_elements_size: 100,
                max_versioning_elements_size: 100,
                thread_count: THREAD_COUNT,
                max_ledger_backups: 10,
            };
            let mut db_backup_2_opts = MassaDB::default_db_opts();
            db_backup_2_opts.create_if_missing(false);
            let db_backup_2 = Arc::new(RwLock::new(Box::new(
                MassaDB::new_with_options(db_backup_2_config, db_backup_2_opts.clone()).unwrap(),
            )
                as Box<(dyn MassaDBController + 'static)>));

            assert_eq!(db_backup_2.read().get_xof_db_hash(), hash_2);
            assert_ne!(db_backup_2.read().get_xof_db_hash(), hash_1);
            assert_ne!(
                Hash::compute_from(db_backup_2.read().get_xof_db_hash().to_bytes()),
                initial_hash()
            );
        }
    }

    #[test]
    fn test_backup_rotation() {
        // 1- Init a db
        // 2- Loop (add data, backup) until a rotation occurs
        // 3- Init from backups + checks

        // Init
        let temp_dir_db = tempdir().expect("Unable to create a temp folder");
        // println!("temp_dir_db: {:?}", temp_dir_db);
        let db_config = MassaDBConfig {
            path: temp_dir_db.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };
        let mut db_opts = MassaDB::default_db_opts();
        // Additional checks (only for testing)
        db_opts.set_paranoid_checks(true);

        let db = Arc::new(RwLock::new(Box::new(
            MassaDB::new_with_options(db_config.clone(), db_opts.clone()).unwrap(),
        )
            as Box<(dyn MassaDBController + 'static)>));

        let mut backups = BTreeMap::default();

        for i in 0..(db_config.max_ledger_backups + 1) {
            let slot = Slot::new(i, 0);
            let i_ = i as u8;
            let batch = DBBatch::from([(vec![i_], Some(vec![i_ + 10]))]);
            let versioning_batch = DBBatch::from([(vec![i_ + 1], Some(vec![i_ + 20]))]);

            let mut guard = db.write();
            guard.write_batch(batch.clone(), versioning_batch.clone(), Some(slot));
            drop(guard);

            let xof = db.read().get_xof_db_hash();
            let backup = db.read().backup_db(slot);

            backups.insert(slot, (xof, backup));
        }

        let mut db_opts_no_create = db_opts.clone();
        db_opts_no_create.create_if_missing(false);

        for i in 0..(db_config.max_ledger_backups + 1) {
            let slot = Slot::new(i, 0);
            let (backup_xof, backup_path) = backups.get(&slot).unwrap();

            // println!(
            //     "Checking backup_path: {:?} -> exists: {}",
            //     backup_path,
            //     backup_path.exists()
            // );
            let db_backup_config = MassaDBConfig {
                path: backup_path.clone(),
                max_history_length: 100,
                max_final_state_elements_size: 100,
                max_versioning_elements_size: 100,
                thread_count: THREAD_COUNT,
                max_ledger_backups: 10,
            };
            // let db_backup_2_opts = MassaDB::default_db_opts();

            let db_backup_2 = match i {
                0 => {
                    let _db = MassaDB::new_with_options(
                        db_backup_config.clone(),
                        db_opts_no_create.clone(),
                    );
                    // backup (index 0) has been removed because we exceed MAX_BACKUPS_TO_KEEP
                    assert!(_db.is_err());
                    continue;
                }
                _ => {
                    let _db = MassaDB::new_with_options(
                        db_backup_config.clone(),
                        db_opts_no_create.clone(),
                    );
                    Arc::new(RwLock::new(
                        Box::new(_db.unwrap()) as Box<(dyn MassaDBController + 'static)>
                    ))
                }
            };

            assert_eq!(db_backup_2.read().get_xof_db_hash(), *backup_xof);
            assert_ne!(
                Hash::compute_from(db_backup_2.read().get_xof_db_hash().to_bytes()),
                initial_hash()
            );
        }
    }

    #[test]
    fn test_db_stream() {
        // Init db + add data
        // Stream Started
        // Stream Ongoing
        // Stream Finished
        // Stream edge cases

        let temp_dir_db = tempdir().expect("Unable to create a temp folder");
        // println!("temp_dir_db: {:?}", temp_dir_db);
        let db_config = MassaDBConfig {
            path: temp_dir_db.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };
        let mut db_opts = MassaDB::default_db_opts();
        // Additional checks (only for testing)
        db_opts.set_paranoid_checks(true);

        let _db = MassaDB::new_with_options(db_config, db_opts.clone()).unwrap();
        let db = Arc::new(RwLock::new(
            Box::new(_db) as Box<(dyn MassaDBController + 'static)>
        ));

        // Add data (at slot 1)
        let batch_key_1 = vec![1, 2, 3];
        let batch_value_1 = vec![4, 5, 6];
        let batch = DBBatch::from([(batch_key_1.clone(), Some(batch_value_1))]);
        let versioning_batch = DBBatch::from([(vec![10, 20, 30], Some(vec![127, 128, 254, 255]))]);
        let slot_1 = Slot::new(1, 0);
        let mut guard = db.write();
        guard.write_batch(batch, versioning_batch, Some(slot_1));
        drop(guard);

        // Add data 2 (at slot 2)
        let batch_key_2 = vec![11, 22, 33];
        let batch_value_2 = vec![44, 55, 66];
        let batch = DBBatch::from([(batch_key_2.clone(), Some(batch_value_2.clone()))]);
        let versioning_batch = DBBatch::from([(vec![12, 23, 34], Some(vec![255, 254, 253]))]);
        let slot_2 = Slot::new(2, 0);
        let mut guard = db.write();
        guard.write_batch(batch, versioning_batch, Some(slot_2));
        drop(guard);

        // Stream using StreamingStep::Started
        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Started;
        let stream_batch_ = db.read().get_batch_to_stream(&last_state_step, None);
        let stream_batch = stream_batch_.unwrap();
        // Here we retrieved the whole db content (see config.max_new_elements)
        // assert_eq!(stream_batch.new_elements, dump_column(db_.clone(), "state"));
        assert_eq!(stream_batch.new_elements, dump_column(db.clone(), "state"));
        assert_eq!(stream_batch.updates_on_previous_elements, BTreeMap::new());
        assert_eq!(stream_batch.change_id, slot_2);

        // Same using StreamingStep::Ongoing, starting from batch_key_1
        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Ongoing(batch_key_1);
        let stream_batch_ = db
            .read()
            .get_batch_to_stream(&last_state_step, Some(slot_2));
        let stream_batch = stream_batch_.unwrap();
        // println!("stream_batch: {:?}", stream_batch);
        assert_eq!(
            stream_batch.new_elements,
            BTreeMap::from([(batch_key_2, batch_value_2)])
        );
        assert_eq!(stream_batch.updates_on_previous_elements, BTreeMap::new());
        assert_eq!(stream_batch.change_id, slot_2);

        // StreamingStep::Finished
        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Finished(None);
        let stream_batch = db
            .read()
            .get_batch_to_stream(&last_state_step, Some(slot_2));

        assert_eq!(stream_batch.unwrap().new_elements, BTreeMap::new());

        // Edge cases

        // Stream from the future
        let stream_batch = db
            .read()
            .get_batch_to_stream(&StreamingStep::Ongoing(vec![]), Some(Slot::new(5, 0)));
        // println!("stream_batch: {:?}", stream_batch);
        assert_matches!(stream_batch, Err(MassaDBError::CacheMissError(..)));
        assert!(stream_batch.err().unwrap().to_string().contains("future"));

        //
        let stream_batch = db
            .read()
            .get_batch_to_stream(&StreamingStep::Finished(None), None);
        // println!("stream_batch: {:?}", stream_batch);
        assert_matches!(stream_batch, Err(TimeError(..)));
    }

    #[test]
    fn test_db_stream_versioning() {
        // Same as test_db_stream but for versioning

        let temp_dir_db = tempdir().expect("Unable to create a temp folder");
        // println!("temp_dir_db: {:?}", temp_dir_db);
        let db_config = MassaDBConfig {
            path: temp_dir_db.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };
        let mut db_opts = MassaDB::default_db_opts();
        // Additional checks (only for testing)
        db_opts.set_paranoid_checks(true);

        let _db = MassaDB::new_with_options(db_config, db_opts.clone()).unwrap();
        let db = Arc::new(RwLock::new(
            Box::new(_db) as Box<(dyn MassaDBController + 'static)>
        ));

        // let db_ = db.read().get_db();

        // Add data
        let batch_key_1 = vec![1, 2, 3];
        let batch_value_1 = vec![4, 5, 6];
        let batch_1 = DBBatch::from([(batch_key_1, Some(batch_value_1))]);
        let batch_v_key_1 = vec![10, 20, 30];
        let batch_v_value_1 = vec![127, 128, 254, 255];
        let versioning_batch_1 = DBBatch::from([(batch_v_key_1.clone(), Some(batch_v_value_1))]);
        let slot_1 = Slot::new(1, 0);
        let mut guard = db.write();
        guard.write_batch(batch_1, versioning_batch_1, Some(slot_1));
        drop(guard);

        // Add data
        let batch_key_2 = vec![11, 22, 33];
        let batch_value_2 = vec![44, 55, 66];
        let batch_2 = DBBatch::from([(batch_key_2, Some(batch_value_2))]);
        let batch_v_key_2 = vec![12, 23, 34];
        let batch_v_value_2 = vec![255, 254, 253];
        let versioning_batch_2 =
            DBBatch::from([(batch_v_key_2.clone(), Some(batch_v_value_2.clone()))]);
        let slot_2 = Slot::new(2, 0);
        let mut guard = db.write();
        guard.write_batch(batch_2, versioning_batch_2, Some(slot_2));
        drop(guard);

        // Stream using StreamingStep::Started
        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Started;
        let stream_batch_ = db
            .read()
            .get_versioning_batch_to_stream(&last_state_step, None);
        let stream_batch = stream_batch_.unwrap();
        // Here we retrieved the whole db content (see config.max_new_elements )
        assert_eq!(
            stream_batch.new_elements,
            dump_column(db.clone(), "versioning")
        );
        assert_eq!(stream_batch.updates_on_previous_elements, BTreeMap::new());
        assert_eq!(stream_batch.change_id, slot_2);

        // Same using StreamingStep::Ongoing, starting from batch_key_1
        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Ongoing(batch_v_key_1);
        let stream_batch_ = db
            .read()
            .get_versioning_batch_to_stream(&last_state_step, Some(slot_2));
        let stream_batch = stream_batch_.unwrap();
        // println!("stream_batch: {:?}", stream_batch);
        assert_eq!(
            stream_batch.new_elements,
            BTreeMap::from([(batch_v_key_2, batch_v_value_2)])
        );
        assert_eq!(stream_batch.updates_on_previous_elements, BTreeMap::new());
        assert_eq!(stream_batch.change_id, slot_2);

        // StreamingStep::Finished
        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Finished(None);
        let stream_batch = db
            .read()
            .get_batch_to_stream(&last_state_step, Some(slot_2));

        assert_eq!(stream_batch.unwrap().new_elements, BTreeMap::new());
    }

    #[test]
    fn test_db_stream_2() {
        // Init db + add data
        // Stream (Ongoing) with previous data updated

        let temp_dir_db = tempdir().expect("Unable to create a temp folder");
        println!("temp_dir_db: {:?}", temp_dir_db);
        let db_config = MassaDBConfig {
            path: temp_dir_db.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 10,
            max_versioning_elements_size: 10,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };
        let mut db_opts = MassaDB::default_db_opts();
        // Additional checks (only for testing)
        db_opts.set_paranoid_checks(true);

        let _db = MassaDB::new_with_options(db_config, db_opts.clone()).unwrap();
        let db = Arc::new(RwLock::new(
            Box::new(_db) as Box<(dyn MassaDBController + 'static)>
        ));

        // Add data 1 (at slot 1)
        let batch_key_1 = vec![1, 2, 3];
        let batch_value_1 = vec![4, 5, 6];
        let batch_1 = DBBatch::from([(batch_key_1.clone(), Some(batch_value_1.clone()))]);
        let versioning_batch_1 =
            DBBatch::from([(vec![10, 20, 30], Some(vec![127, 128, 254, 255]))]);
        let slot_1 = Slot::new(1, 0);
        let mut guard = db.write();
        guard.write_batch(batch_1, versioning_batch_1, Some(slot_1));
        drop(guard);

        // Add data 2 (at slot 1)
        let batch_key_2 = vec![11, 22, 33];
        let batch_value_2 = vec![44, 55, 66];
        let batch_2 = DBBatch::from([(batch_key_2.clone(), Some(batch_value_2.clone()))]);
        let versioning_batch_2 = DBBatch::from([(vec![12, 23, 34], Some(vec![255, 254, 253]))]);
        let slot_2 = Slot::new(2, 0);
        let mut guard = db.write();
        guard.write_batch(batch_2, versioning_batch_2, Some(slot_1));
        drop(guard);

        // Stream using StreamingStep::Started
        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Started;
        let stream_batch_ = db.read().get_batch_to_stream(&last_state_step, None);
        let stream_batch = stream_batch_.unwrap();
        assert_eq!(
            stream_batch.new_elements,
            BTreeMap::from([(batch_key_1.clone(), batch_value_1)])
        );
        assert_eq!(stream_batch.updates_on_previous_elements, BTreeMap::new());
        assert_eq!(stream_batch.change_id, slot_1);

        // Update data 1 (at slot 2)
        let batch_value_1b = vec![94, 95, 96];
        let batch_1b = DBBatch::from([(batch_key_1.clone(), Some(batch_value_1b.clone()))]);
        let versioning_batch_1b = DBBatch::new();
        let mut guard = db.write();
        guard.write_batch(batch_1b, versioning_batch_1b, Some(slot_2));
        drop(guard);

        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Ongoing(batch_key_1.clone());
        let stream_batch_ = db
            .read()
            .get_batch_to_stream(&last_state_step, Some(slot_1));
        let stream_batch = stream_batch_.unwrap();
        assert_eq!(
            stream_batch.new_elements,
            BTreeMap::from([(batch_key_2, batch_value_2)])
        );
        assert_eq!(
            stream_batch.updates_on_previous_elements,
            BTreeMap::from([(batch_key_1, Some(batch_value_1b))])
        );
        assert_eq!(stream_batch.change_id, slot_2);
    }

    #[test]
    fn test_db_stream_3() {
        // Init db + add data
        // Stream until finished
        // Add + Update some values
        // Stream Finished
        // Note: This happens in the bootstrap process where the state is streamed then
        //       the consensus is streamed, but during that, updates to the state must be
        //       streamed as well

        let temp_dir_db = tempdir().expect("Unable to create a temp folder");
        // println!("temp_dir_db: {:?}", temp_dir_db);
        let db_config = MassaDBConfig {
            path: temp_dir_db.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 20,
            max_versioning_elements_size: 20,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };
        let mut db_opts = MassaDB::default_db_opts();
        // Additional checks (only for testing)
        db_opts.set_paranoid_checks(true);

        let _db = MassaDB::new_with_options(db_config, db_opts.clone()).unwrap();
        let db = Arc::new(RwLock::new(
            Box::new(_db) as Box<(dyn MassaDBController + 'static)>
        ));

        // Add data 1 (at slot 1)
        let batch_key_1 = vec![1, 2, 3];
        let batch_value_1 = vec![4, 5, 6];
        let batch_1 = DBBatch::from([(batch_key_1.clone(), Some(batch_value_1.clone()))]);
        let versioning_batch_1 =
            DBBatch::from([(vec![10, 20, 30], Some(vec![127, 128, 254, 255]))]);
        let slot_1 = Slot::new(1, 0);
        let mut guard = db.write();
        guard.write_batch(batch_1, versioning_batch_1, Some(slot_1));
        drop(guard);

        // Add data 2 (at slot 1)
        let batch_key_2 = vec![11, 22, 33];
        let batch_value_2 = vec![44, 55, 66];
        let batch_2 = DBBatch::from([(batch_key_2.clone(), Some(batch_value_2.clone()))]);
        let versioning_batch_2 = DBBatch::from([(vec![12, 23, 34], Some(vec![255, 254, 253]))]);
        let slot_2 = Slot::new(2, 0);
        let mut guard = db.write();
        guard.write_batch(batch_2, versioning_batch_2, Some(slot_1));
        drop(guard);

        // Stream using StreamingStep::Started
        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Started;
        let stream_batch_ = db.read().get_batch_to_stream(&last_state_step, None);
        let stream_batch = stream_batch_.unwrap();
        assert_eq!(
            stream_batch.new_elements,
            BTreeMap::from([
                (batch_key_1.clone(), batch_value_1.clone()),
                (batch_key_2.clone(), batch_value_2)
            ])
        );
        assert_eq!(stream_batch.updates_on_previous_elements, BTreeMap::new());
        assert_eq!(stream_batch.change_id, slot_1);

        // Now updates some value (at slot 2)
        let batch_value_2b = vec![94, 95, 96];
        let batch_2b = DBBatch::from([(batch_key_2.clone(), Some(batch_value_2b.clone()))]);
        let versioning_batch_2b = DBBatch::new();
        let mut guard = db.write();
        guard.write_batch(batch_2b, versioning_batch_2b, Some(slot_2));
        drop(guard);

        // Add data (at slot 3)
        let batch_key_3 = vec![61, 62, 63];
        let batch_value_3 = vec![4, 5, 6, 7];
        let batch_3 = DBBatch::from([(batch_key_3.clone(), Some(batch_value_3.clone()))]);
        let versioning_batch_3 = DBBatch::new();
        let slot_3 = Slot::new(3, 0);
        let mut guard = db.write();
        guard.write_batch(batch_3, versioning_batch_3, Some(slot_3));
        drop(guard);

        // Stream using StreamingStep::Finished
        let last_state_step: StreamingStep<Vec<u8>> =
            StreamingStep::Finished(Some(batch_key_2.clone()));
        let stream_batch_ = db
            .read()
            .get_batch_to_stream(&last_state_step, Some(slot_1));
        let stream_batch = stream_batch_.unwrap();

        // Note: new_elements is empty, everything is on updates_on_previous_elements
        //       new_elements are fulfilled only if StreamingStep != StreamingStep::Finished
        assert!(stream_batch.new_elements.is_empty());
        assert_eq!(
            stream_batch.updates_on_previous_elements,
            BTreeMap::from([
                (batch_key_1, Some(batch_value_1)),
                (batch_key_2, Some(batch_value_2b)),
                (batch_key_3.clone(), Some(batch_value_3))
            ])
        );

        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Finished(Some(batch_key_3));
        let stream_batch_ = db
            .read()
            .get_batch_to_stream(&last_state_step, Some(slot_3));
        let stream_batch = stream_batch_.unwrap();

        // No more updates and new elements -> all empty
        assert!(stream_batch.new_elements.is_empty());
        assert!(stream_batch.updates_on_previous_elements.is_empty());
    }

    #[test]
    fn test_db_err_if_changes_not_available() {
        // Init db + add data
        // Stream Slot 1, should succeed
        // Add changes for N slots, N <= max_history_length
        // Stream Slot 2, should succeed
        // Add changes for M slots, M > max_history_length
        // Stream Slot 3, should return error because we don't have changes for slot 2

        let temp_dir_db = tempdir().expect("Unable to create a temp folder");
        // println!("temp_dir_db: {:?}", temp_dir_db);
        let db_config = MassaDBConfig {
            path: temp_dir_db.path().to_path_buf(),
            max_history_length: 4,
            max_final_state_elements_size: 20,
            max_versioning_elements_size: 20,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };

        let slot_1 = Slot::new(1, 0);
        let slot_2 = Slot::new(1, 2);
        let slot_3 = Slot::new(1, 8);

        let mut db_opts = MassaDB::default_db_opts();
        // Additional checks (only for testing)
        db_opts.set_paranoid_checks(true);

        let _db = MassaDB::new_with_options(db_config, db_opts.clone()).unwrap();
        let db = Arc::new(RwLock::new(
            Box::new(_db) as Box<(dyn MassaDBController + 'static)>
        ));

        // Add data 1 (at slot 1)
        let batch_key_1 = vec![1, 2, 3];
        let batch_value_1 = vec![4, 5, 6];
        let batch_1 = DBBatch::from([(batch_key_1.clone(), Some(batch_value_1.clone()))]);
        let versioning_batch_1 =
            DBBatch::from([(vec![10, 20, 30], Some(vec![127, 128, 254, 255]))]);
        let mut guard = db.write();
        guard.write_batch(batch_1, versioning_batch_1, Some(slot_1));
        drop(guard);

        // Add data 2 (at slot 1)
        let batch_key_2 = vec![11, 22, 33];
        let batch_value_2 = vec![44, 55, 66];
        let batch_2 = DBBatch::from([(batch_key_2.clone(), Some(batch_value_2.clone()))]);
        let versioning_batch_2 = DBBatch::from([(vec![12, 23, 34], Some(vec![255, 254, 253]))]);
        let mut guard = db.write();
        guard.write_batch(batch_2, versioning_batch_2, Some(slot_1));
        drop(guard);

        // Stream using StreamingStep::Started
        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Started;
        let stream_batch_ = db.read().get_batch_to_stream(&last_state_step, None);
        let stream_batch = stream_batch_.unwrap();
        assert_eq!(
            stream_batch.new_elements,
            BTreeMap::from([
                (batch_key_1.clone(), batch_value_1.clone()),
                (batch_key_2.clone(), batch_value_2)
            ])
        );
        assert_eq!(stream_batch.updates_on_previous_elements, BTreeMap::new());
        assert_eq!(stream_batch.change_id, slot_1);

        // Now updates some values for each slot until slot 2 (included)
        let mut cur_slot = slot_1.get_next_slot(THREAD_COUNT).unwrap();
        while cur_slot <= slot_2 {
            let batch_key = vec![(cur_slot.period % u8::MAX as u64) as u8];
            let batch_value = vec![cur_slot.thread];
            let batch = DBBatch::from([(batch_key.clone(), Some(batch_value.clone()))]);
            let versioning_batch = DBBatch::from([(
                vec![(cur_slot.period % u8::MAX as u64) as u8],
                Some(vec![cur_slot.thread]),
            )]);
            let mut guard = db.write();
            guard.write_batch(batch, versioning_batch, Some(cur_slot));
            drop(guard);
            cur_slot = cur_slot.get_next_slot(THREAD_COUNT).unwrap();
        }

        // Stream using StreamingStep::Finished
        let last_state_step: StreamingStep<Vec<u8>> =
            StreamingStep::Finished(Some(batch_key_2.clone()));
        let stream_batch_ = db
            .read()
            .get_batch_to_stream(&last_state_step, Some(slot_1));
        assert!(stream_batch_.is_ok());

        // Now updates some values for each slot until slot 3 (included)
        // There are more slots between slot_2 and slot_3 than max_history_length, so we will CacheMiss
        let mut cur_slot = slot_2.get_next_slot(THREAD_COUNT).unwrap();
        while cur_slot <= slot_3 {
            let batch_key = vec![(cur_slot.period % 256) as u8];
            let batch_value = vec![cur_slot.thread];
            let batch = DBBatch::from([(batch_key.clone(), Some(batch_value.clone()))]);
            let versioning_batch = DBBatch::from([(
                vec![(cur_slot.period % 256) as u8],
                Some(vec![cur_slot.thread]),
            )]);
            let mut guard = db.write();
            guard.write_batch(batch, versioning_batch, Some(cur_slot));
            drop(guard);
            cur_slot = cur_slot.get_next_slot(THREAD_COUNT).unwrap();
        }

        // Stream using StreamingStep::Ongoing
        let last_state_step: StreamingStep<Vec<u8>> = StreamingStep::Ongoing(batch_key_2.clone());
        let stream_batch_ = db
            .read()
            .get_batch_to_stream(&last_state_step, Some(slot_2));
        assert!(stream_batch_.is_err());
        assert!(stream_batch_.unwrap_err().to_string().contains("all our changes are strictly after last_change_id, we can't be sure we did not miss any"));

        // Stream using StreamingStep::Finished
        let last_state_step: StreamingStep<Vec<u8>> =
            StreamingStep::Finished(Some(batch_key_2.clone()));
        let stream_batch_ = db
            .read()
            .get_batch_to_stream(&last_state_step, Some(slot_2));
        assert!(stream_batch_.is_err());
        assert!(stream_batch_.unwrap_err().to_string().contains("all our changes are strictly after last_change_id, we can't be sure we did not miss any"));
    }
}
