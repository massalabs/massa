use massa_hash::Hash;
use rocksdb::{ColumnFamilyDescriptor, Options, WriteBatch, DB};
use std::{collections::BTreeMap, path::PathBuf};

use crate::{
    ASYNC_POOL_CF, ASYNC_POOL_HASH_KEY, CF_ERROR, CRUD_ERROR, CYCLE_HISTORY_CF,
    CYCLE_HISTORY_HASH_KEY, DEFERRED_CREDITS_CF, DEFERRED_CREDITS_HASH_KEY, EXECUTED_OPS_CF,
    EXECUTED_OPS_HASH_KEY, LEDGER_CF, LEDGER_HASH_KEY, METADATA_CF, OPEN_ERROR,
};

/// Batch containing write operations to perform on disk and cache for hash computing
pub struct DBBatch {
    // Rocksdb write batch
    pub write_batch: WriteBatch,
    // Ledger hash state in the current batch
    pub ledger_hash: Option<Hash>,
    // Async pool hash state in the current batch
    pub async_pool_hash: Option<Hash>,
    // cycle_history hash state in the current batch
    pub cycle_history_hash: Option<Hash>,
    // deferred credits hash state in the current batch
    pub deferred_credits_hash: Option<Hash>,
    // executed ops hash state in the current batch
    pub executed_ops_hash: Option<Hash>,
    // Added entry hashes in the current batch
    pub aeh_list: BTreeMap<Vec<u8>, Hash>,
}

impl DBBatch {
    pub fn new(
        ledger_hash: Option<Hash>,
        async_pool_hash: Option<Hash>,
        cycle_history_hash: Option<Hash>,
        deferred_credits_hash: Option<Hash>,
        executed_ops_hash: Option<Hash>,
    ) -> Self {
        Self {
            write_batch: WriteBatch::default(),
            ledger_hash,
            async_pool_hash,
            cycle_history_hash,
            deferred_credits_hash,
            executed_ops_hash,
            aeh_list: BTreeMap::new(),
        }
    }
}

pub fn write_batch(db: &DB, mut batch: DBBatch) {
    let (
        async_pool_hash,
        cycle_history_hash,
        deferred_credits_hash,
        executed_ops_hash,
        ledger_hash,
    ) = (
        batch.async_pool_hash,
        batch.cycle_history_hash,
        batch.deferred_credits_hash,
        batch.executed_ops_hash,
        batch.ledger_hash,
    );
    try_put_hash_in_batch(db, &mut batch, ASYNC_POOL_HASH_KEY, async_pool_hash);
    try_put_hash_in_batch(db, &mut batch, CYCLE_HISTORY_HASH_KEY, cycle_history_hash);
    try_put_hash_in_batch(
        db,
        &mut batch,
        DEFERRED_CREDITS_HASH_KEY,
        deferred_credits_hash,
    );
    try_put_hash_in_batch(db, &mut batch, EXECUTED_OPS_HASH_KEY, executed_ops_hash);
    try_put_hash_in_batch(db, &mut batch, LEDGER_HASH_KEY, ledger_hash);

    db.write(batch.write_batch).expect(CRUD_ERROR);
}

fn try_put_hash_in_batch(db: &DB, batch: &mut DBBatch, key: &[u8], hash: Option<Hash>) {
    let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);

    if let Some(hash) = hash {
        batch.write_batch.put_cf(handle, key, hash.to_bytes());
    }
}

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
            ColumnFamilyDescriptor::new(CYCLE_HISTORY_CF, Options::default()),
            ColumnFamilyDescriptor::new(DEFERRED_CREDITS_CF, Options::default()),
            ColumnFamilyDescriptor::new(EXECUTED_OPS_CF, Options::default()),
        ],
    )
    .expect(OPEN_ERROR)
}

// TODO: Add other utility functions
// TODO: Move some functions from ledger_db to here
// E.g. get and set slot, etc.
