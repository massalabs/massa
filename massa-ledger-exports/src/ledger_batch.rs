use massa_hash::Hash;
use rocksdb::WriteBatch;
use std::collections::BTreeMap;

/// Batch containing write operations to perform on disk and cache for the ledger hash computing
pub struct LedgerBatch {
    // Rocksdb write batch
    pub write_batch: WriteBatch,
    // Ledger hash state in the current batch
    pub ledger_hash: Option<Hash>,
    // Ledger hash state in the current batch
    pub async_pool_hash: Option<Hash>,
    // Added entry hashes in the current batch
    pub aeh_list: BTreeMap<Vec<u8>, Hash>,
}

impl LedgerBatch {
    pub fn new(ledger_hash: Option<Hash>, async_pool_hash: Option<Hash>) -> Self {
        Self {
            write_batch: WriteBatch::default(),
            ledger_hash,
            async_pool_hash,
            aeh_list: BTreeMap::new(),
        }
    }
}
