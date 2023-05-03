use crate::{
    CF_ERROR, CRUD_ERROR, METADATA_CF, OPEN_ERROR, SLOT_DESER_ERROR, SLOT_KEY, STATE_CF,
    STATE_HASH_ERROR, STATE_HASH_INITIAL_BYTES, STATE_HASH_KEY,
};
use massa_hash::Hash;
use massa_models::{
    error::ModelsError,
    slot::{Slot, SlotDeserializer, SlotSerializer},
};
use massa_serialization::{DeserializeError, Deserializer, Serializer};

use rocksdb::{
    checkpoint::Checkpoint, ColumnFamily, ColumnFamilyDescriptor, IteratorMode, Options,
    WriteBatch, DB,
};
use std::{collections::BTreeMap, ops::Bound, path::PathBuf};

#[derive(Debug)]
pub struct MassaDB(pub DB);

impl MassaDB {
    /// Returns a new `MassaDB` instance
    pub fn new(path: PathBuf) -> MassaDB {
        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        let db = DB::open_cf_descriptors(
            &db_opts,
            path,
            vec![
                ColumnFamilyDescriptor::new(STATE_CF, Options::default()),
                ColumnFamilyDescriptor::new(METADATA_CF, Options::default()),
            ],
        )
        .expect(OPEN_ERROR);

        MassaDB(db)
    }

    /// Creates a new hard copy of the DB, for the given slot
    pub fn backup_db(&self, slot: Slot) {
        let db = &self.0;
        let mut subpath = String::from("backup_");
        subpath.push_str(slot.period.to_string().as_str());
        subpath.push('_');
        subpath.push_str(slot.thread.to_string().as_str());

        Checkpoint::new(db)
            .expect("Cannot init checkpoint")
            .create_checkpoint(db.path().join(subpath))
            .expect("Failed to create checkpoint");
    }

    /// Writes the batch to the DB
    pub fn write_batch(&self, mut batch: DBBatch) {
        let db = &self.0;
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);
        batch
            .write_batch
            .put_cf(handle, STATE_HASH_KEY, batch.state_hash.to_bytes());
        db.write(batch.write_batch).expect(CRUD_ERROR);
    }

    /// Utility function to put / update a key & value and perform the hash XORs
    pub fn put_or_update_entry_value(
        &self,
        handle: &ColumnFamily,
        batch: &mut DBBatch,
        key: Vec<u8>,
        value: &[u8],
    ) {
        let db = &self.0;

        if let Some(added_hash) = batch.aeh_list.get(&key) {
            batch.state_hash ^= *added_hash;
        } else if let Some(prev_bytes) = db.get_pinned_cf(handle, &key).expect(CRUD_ERROR) {
            batch.state_hash ^= Hash::compute_from(&[&key, &prev_bytes[..]].concat());
        }
        let hash = Hash::compute_from(&[&key, value].concat());
        batch.state_hash ^= hash;
        batch.aeh_list.insert(key.clone(), hash);
        batch.write_batch.put_cf(handle, key, value);
    }

    /// Utility function to delete a key & value and perform the hash XORs
    pub fn delete_key(&self, handle: &ColumnFamily, batch: &mut DBBatch, key: Vec<u8>) {
        let db = &self.0;
        if let Some(added_hash) = batch.aeh_list.get(&key) {
            batch.state_hash ^= *added_hash;
        } else if let Some(prev_bytes) = db.get_pinned_cf(handle, &key).expect(CRUD_ERROR) {
            batch.state_hash ^= Hash::compute_from(&[&key, &prev_bytes[..]].concat());
        }
        batch.write_batch.delete_cf(handle, key);
    }

    /// Get the current state hash
    pub fn get_db_hash(&self) -> Hash {
        let db = &self.0;

        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);
        if let Some(state_hash_bytes) = db
            .get_cf(handle, STATE_HASH_KEY)
            .expect(CRUD_ERROR)
            .as_deref()
        {
            Hash::from_bytes(state_hash_bytes.try_into().expect(STATE_HASH_ERROR))
        } else {
            Hash::from_bytes(STATE_HASH_INITIAL_BYTES)
        }
    }

    /// Get the current state hash
    pub fn compute_hash_from_scratch(&self) -> Hash {
        let db = &self.0;

        let handle = db.cf_handle(STATE_CF).expect(CF_ERROR);

        let mut hash = Hash::from_bytes(STATE_HASH_INITIAL_BYTES);

        for (serialized_key, serialized_value) in
            db.iterator_cf(handle, IteratorMode::Start).flatten()
        {
            hash ^= Hash::compute_from(&[serialized_key, serialized_value].concat());
        }

        if let Ok(Some(serialized_value)) = db.get_cf(handle, SLOT_KEY) {
            hash ^= Hash::compute_from(&[SLOT_KEY.to_vec(), serialized_value].concat());
        }

        hash
    }

    pub fn delete_prefix(&self, prefix: &str) {
        let db = &self.0;

        let handle = db.cf_handle(STATE_CF).expect(CF_ERROR);
        let mut batch = DBBatch::new(self.get_db_hash());
        for (serialized_key, _) in db.prefix_iterator_cf(handle, prefix).flatten() {
            self.delete_key(handle, &mut batch, serialized_key.to_vec());
        }
    }
    /// Set the disk slot metadata
    ///
    /// # Arguments
    /// * slot: associated slot of the current ledger
    /// * batch: the given operation batch to update
    pub fn set_slot(&self, slot: Slot, batch: &mut DBBatch) {
        let db = &self.0;
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);
        let mut slot_bytes = Vec::new();
        // Slot serialization never fails
        let slot_serializer = SlotSerializer::new();
        slot_serializer.serialize(&slot, &mut slot_bytes).unwrap();

        self.put_or_update_entry_value(handle, batch, SLOT_KEY.to_vec(), &slot_bytes);
    }

    pub fn get_slot(&self, thread_count: u8) -> Result<Slot, ModelsError> {
        let db = &self.0;
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);

        let Ok(Some(slot_bytes)) = db.get_pinned_cf(handle, SLOT_KEY) else {
            return Err(ModelsError::BufferError(String::from("Could not recover ledger_slot")));
        };

        let slot_deserializer = SlotDeserializer::new(
            (Bound::Included(u64::MIN), Bound::Included(u64::MAX)),
            (Bound::Included(0_u8), Bound::Excluded(thread_count)),
        );

        let (_rest, slot) = slot_deserializer
            .deserialize::<DeserializeError>(&slot_bytes)
            .expect(SLOT_DESER_ERROR);

        Ok(slot)
    }

    /// TODO:
    /// Deserialize the entire DB and check the data. Useful to check after bootstrap.
    pub fn is_db_valid(&self) -> bool {
        todo!();
    }
}

/// Batch containing write operations to perform on disk and cache for hash computing
pub struct DBBatch {
    // Rocksdb write batch
    pub write_batch: WriteBatch,
    // State hash in the current batch
    pub state_hash: Hash,
    // Added entry hashes in the current batch
    pub aeh_list: BTreeMap<Vec<u8>, Hash>,
}

impl DBBatch {
    pub fn new(state_hash: Hash) -> Self {
        Self {
            write_batch: WriteBatch::default(),
            state_hash,
            aeh_list: BTreeMap::new(),
        }
    }
}
