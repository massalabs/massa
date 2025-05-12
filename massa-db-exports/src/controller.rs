use crate::{DBBatch, Key, MassaDBError, StreamBatch, Value};
use massa_hash::{HashXof, HASH_XOF_SIZE_BYTES};
use massa_models::{error::ModelsError, slot::Slot, streaming_step::StreamingStep};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::{fmt::Debug, sync::Arc};

#[cfg(feature = "test-exports")]
use std::collections::BTreeMap;

pub type ShareableMassaDBController = Arc<RwLock<Box<dyn MassaDBController>>>;

/// Controller trait for the MassaDB
/// TODO: MOCK IT WITH MOCKALL. HAVING LIFETIMES ERRORS WITH AUTO MOCK
pub trait MassaDBController: Send + Sync + Debug {
    /// Creates a new hard copy of the DB, for the given slot
    fn backup_db(&self, slot: Slot) -> PathBuf;

    /// Get the current change_id attached to the database.
    fn get_change_id(&self) -> Result<Slot, ModelsError>;

    /// Set the initial change_id. This function should only be called at startup/reset, as it does not batch this set with other changes.
    fn set_initial_change_id(&self, change_id: Slot);

    /// Writes the batch to the DB
    fn write_batch(&mut self, batch: DBBatch, versioning_batch: DBBatch, change_id: Option<Slot>);

    /// Utility function to put / update a key & value in the batch
    fn put_or_update_entry_value(&self, batch: &mut DBBatch, key: Vec<u8>, value: &[u8]);

    /// Utility function to delete a key & value in the batch
    fn delete_key(&self, batch: &mut DBBatch, key: Vec<u8>);

    /// Utility function to delete all keys in a prefix
    fn delete_prefix(&mut self, prefix: &str, handle_str: &str, change_id: Option<Slot>);

    /// Reset the database, and attach it to the given slot.
    fn reset(&mut self, slot: Slot);

    /// Exposes RocksDB's "get_cf" function
    fn get_cf(&self, handle_cf: &str, key: Key) -> Result<Option<Value>, MassaDBError>;

    /// Exposes RocksDB's "multi_get_cf" function
    fn multi_get_cf(&self, query: Vec<(&str, Key)>) -> Vec<Result<Option<Value>, MassaDBError>>;

    /// Exposes RocksDB's "iterator_cf" function, without filling up the read cache
    fn iterator_cf_for_full_db_traversal(
        &self,
        handle_cf: &str,
        mode: MassaIteratorMode,
    ) -> Box<dyn Iterator<Item = (Key, Value)> + '_>;

    /// Exposes RocksDB's "iterator_cf" function
    fn iterator_cf(
        &self,
        handle_cf: &str,
        mode: MassaIteratorMode,
    ) -> Box<dyn Iterator<Item = (Key, Value)> + '_>;

    /// Exposes RocksDB's "prefix_iterator_cf" function
    fn prefix_iterator_cf(
        &self,
        handle_cf: &str,
        prefix: &[u8],
    ) -> Box<dyn Iterator<Item = (Key, Value)> + '_>;

    /// Get the current extended state hash of the database
    fn get_xof_db_hash(&self) -> HashXof<HASH_XOF_SIZE_BYTES>;

    /// Flushes the underlying db.
    fn flush(&self) -> Result<(), MassaDBError>;

    /// Write a stream_batch of database entries received from a bootstrap server
    fn write_batch_bootstrap_client(
        &mut self,
        stream_changes: StreamBatch<Slot>,
        stream_changes_versioning: StreamBatch<Slot>,
    ) -> Result<(StreamingStep<Key>, StreamingStep<Key>), MassaDBError>;

    /// Used for bootstrap servers (get a new batch of data from STATE_CF to stream to the client)
    ///
    /// Returns a StreamBatch<Slot>
    fn get_batch_to_stream(
        &self,
        last_state_step: &StreamingStep<Vec<u8>>,
        last_change_id: Option<Slot>,
    ) -> Result<StreamBatch<Slot>, MassaDBError>;

    /// Used for bootstrap servers (get a new batch of data from VERSIONING_CF to stream to the client)
    ///
    /// Returns a StreamBatch<Slot>
    fn get_versioning_batch_to_stream(
        &self,
        last_versioning_step: &StreamingStep<Vec<u8>>,
        last_change_id: Option<Slot>,
    ) -> Result<StreamBatch<Slot>, MassaDBError>;

    /// Get the total size of the change history and the change versioning history respectively
    fn get_change_history_sizes(&self) -> (usize, usize);

    /// Used in test to compare a prebuilt ledger with a ledger that has been built by the code
    #[cfg(feature = "test-exports")]
    fn get_entire_database(&self) -> Vec<BTreeMap<Vec<u8>, Vec<u8>>>;
}

/// Similar to RocksDB's IteratorMode
pub enum MassaIteratorMode<'a> {
    Start,
    End,
    From(&'a [u8], MassaDirection),
}

/// Similar to RocksDB's Direction
pub enum MassaDirection {
    Forward,
    Reverse,
}
