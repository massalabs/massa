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
    checkpoint::Checkpoint, BlockBasedOptions, Cache, ColumnFamilyDescriptor, Direction,
    IteratorMode, Options, ReadOptions, WriteBatch, WriteBufferManager, DB,
};
use std::{collections::HashMap, path::PathBuf};
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
    pub db: DB,
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
    /// If metrics are enabled, we keep track of the size of the changes associated to each change_id
    pub change_history_sizes: BTreeMap<ChangeID, (usize, usize)>,

    pub secondaries: HashMap<usize, DB>,
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
        i: usize
    ) -> Result<StreamBatch<ChangeID>, MassaDBError> {

        let db = self.secondaries.get(&i).unwrap();
        db.try_catch_up_with_primary().unwrap();

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
            let handle = db.cf_handle(STATE_CF).expect(CF_ERROR);
            let mut read_opt = ReadOptions::default();
            read_opt.fill_cache(false);

            // Creates an iterator from the next element after the last if defined, otherwise initialize it at the first key.
            let db_iterator = match &last_state_step {
                StreamingStep::Ongoing(max_key) => {
                    let mut iter = db.iterator_cf_opt(
                        handle,
                        read_opt,
                        IteratorMode::From(max_key, Direction::Forward),
                    );
                    iter.next();
                    iter
                }
                _ => db
                    .iterator_cf_opt(handle, read_opt, IteratorMode::Start),
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
        i: usize
    ) -> Result<StreamBatch<ChangeID>, MassaDBError> {

        let db = self.secondaries.get(&i).unwrap();
        db.try_catch_up_with_primary().unwrap();

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
            let handle = db.cf_handle(VERSIONING_CF).expect(CF_ERROR);

            // Creates an iterator from the next element after the last if defined, otherwise initialize it at the first key.
            let db_iterator = match &last_versioning_step {
                StreamingStep::Ongoing(max_key) => {
                    let mut iter = db
                        .iterator_cf(handle, IteratorMode::From(max_key, Direction::Forward));
                    iter.next();
                    iter
                }
                _ => db.iterator_cf(handle, IteratorMode::Start),
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
        if let Some(change_id) = change_id.as_ref() {
            if *change_id < self.get_change_id().expect(CHANGE_ID_DESER_ERROR) {
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

        // If metrics are enabled, we keep track of the size of the changes (for state changes, and then for versioning changes)
        if self.config.enable_metrics {
            let changes_size;
            let changes_versioning_size;
            {
                let mut current_batch_guard = self.current_batch.lock();
                changes_size = current_batch_guard.size_in_bytes();

                // in versioning_changes, we have the data that we do not want to include in hash
                // e.g everything that is not in 'Active' state (so hashes remain compatibles)
                for (key, value) in versioning_changes.iter() {
                    if let Some(value) = value {
                        current_batch_guard.put_cf(handle_versioning, key, value);
                    } else {
                        current_batch_guard.delete_cf(handle_versioning, key);
                    }
                }

                changes_versioning_size = current_batch_guard
                    .size_in_bytes()
                    .saturating_sub(changes_size);
            }

            match self
                .change_history_sizes
                .entry(self.get_change_id().expect(CHANGE_ID_DESER_ERROR))
            {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert((changes_size, changes_versioning_size));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().0 += changes_size;
                    entry.get_mut().1 += changes_versioning_size;
                }
            }
        } else {
            let mut current_batch_guard = self.current_batch.lock();
            // in versioning_changes, we have the data that we do not want to include in hash
            // e.g everything that is not in 'Active' state (so hashes remain compatibles)
            for (key, value) in versioning_changes.iter() {
                if let Some(value) = value {
                    current_batch_guard.put_cf(handle_versioning, key, value);
                } else {
                    current_batch_guard.delete_cf(handle_versioning, key);
                }
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
            self.change_history_sizes.clear();
        }

        while self.change_history.len() > self.config.max_history_length {
            self.change_history.pop_first();
            self.change_history_sizes.pop_first();
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

    /// Sets various db options for RocksDB, e.g. to minimize memory usage
    pub fn default_db_opts() -> Options {
        // 1. Basic options
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        // 2. Limit the max number of open files (which decreases memory requirements)
        db_opts.set_max_open_files(820);

        // 3. Set block (read cache) related options
        let mut block_opts = BlockBasedOptions::default();

        // Default block cache is 8 Mb, but here we will also include filters and indexes
        let cache = Cache::new_lru_cache(1 * 1024 * 1024 * 1024); // 1 GB
        block_opts.set_block_cache(&cache);

        block_opts.set_block_size(16 * 1024); // 16 KB instead of default 4 KB, to reduce memory usage

        // Set hybrid bloom and ribbon filter, to reduce both memory and cpu usage, optimized it for memory, and add to cache
        block_opts.set_hybrid_ribbon_filter(5.0, 2);
        block_opts.set_optimize_filters_for_memory(true);
        block_opts.set_cache_index_and_filter_blocks(true);
        block_opts.set_pin_l0_filter_and_index_blocks_in_cache(true);
        block_opts.set_pin_top_level_index_and_filter(true);

        db_opts.set_block_based_table_factory(&block_opts);

        // 4. Set memtables (write cache) options
        // Use a global memtable budget of 512 MB for the DB
        let wbm = WriteBufferManager::new_write_buffer_manager(512 * 1024 * 1024, true);
        db_opts.set_write_buffer_manager(&wbm);

        // Also, for safety, limit each memtable to 128 MB and at most 4 of them
        db_opts.set_write_buffer_size(128 * 1024 * 1024);
        db_opts.set_max_write_buffer_number(4);
        db_opts
    }

    /// Returns a new `MassaDB` instance given a config and RocksDB options
    fn new_with_options(config: MassaDBConfig, db_opts: Options) -> Result<Self, rocksdb::Error> {
        let db = DB::open_cf_descriptors(
            &db_opts.clone(),
            &config.path,
            vec![
                ColumnFamilyDescriptor::new(STATE_CF, db_opts.clone()),
                ColumnFamilyDescriptor::new(METADATA_CF, db_opts.clone()),
                ColumnFamilyDescriptor::new(VERSIONING_CF, db_opts),
            ],
        )?;

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
            change_history_sizes: BTreeMap::new(),
            change_id_serializer: SlotSerializer::new(),
            change_id_deserializer,
            current_batch,
            secondaries: HashMap::new(),
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

    fn new_secondary(&mut self, i: usize) {
        let path = self.config.path.clone();
        let secondary_path = path.join(format!("secondary_{}", i));

        let db_opts = Self::default_db_opts();
        let db = DB::open_cf_descriptors_as_secondary(
            &db_opts.clone(),
            &self.config.path,
            &secondary_path,
            vec![
                ColumnFamilyDescriptor::new(STATE_CF, db_opts.clone()),
                ColumnFamilyDescriptor::new(METADATA_CF, db_opts.clone()),
                ColumnFamilyDescriptor::new(VERSIONING_CF, db_opts),
            ],
        )
        .expect(OPEN_ERROR);

        self.secondaries.insert(i, db);        
    }

    fn remove_secondary(&mut self, i: usize) {
        self.secondaries.remove(&i);
    }

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
        let mut read_opts = ReadOptions::default();
        read_opts.fill_cache(false);

        Box::new(
            db.iterator_cf_opt(handle, read_opts, rocksdb_mode)
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
        i: usize,
    ) -> Result<StreamBatch<Slot>, MassaDBError> {
        self.get_batch_to_stream(last_state_step, last_change_id, i)
    }

    /// Used for bootstrap servers (get a new batch of data from VERSIONING_CF to stream to the client)
    ///
    /// Returns a StreamBatch<Slot>
    fn get_versioning_batch_to_stream(
        &self,
        last_versioning_step: &StreamingStep<Vec<u8>>,
        last_change_id: Option<Slot>,
        i: usize
    ) -> Result<StreamBatch<Slot>, MassaDBError> {
        self.get_versioning_batch_to_stream(last_versioning_step, last_change_id, i)
    }

    fn get_change_history_sizes(&self) -> (usize, usize) {
        self.change_history_sizes.values().fold(
            (0, 0),
            |(acc_state, acc_version), &(state, version)| {
                (acc_state + state, acc_version + version)
            },
        )
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
