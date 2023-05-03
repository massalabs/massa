// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::sync::Arc;

use massa_db::MassaDB;
use parking_lot::RwLock;
/// This file defines testing tools related to the configuration
use tempfile::TempDir;

use crate::{ledger_db::LedgerDB, FinalLedger};
use massa_models::config::{
    LEDGER_PART_SIZE_MESSAGE_BYTES, MAX_DATASTORE_KEY_LENGTH, THREAD_COUNT,
};

/// Default value of `FinalLedger` used for tests
impl Default for FinalLedger {
    fn default() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let db = MassaDB::new(temp_dir.path().to_path_buf());
        let db = LedgerDB::new(
            Arc::new(RwLock::new(db)),
            THREAD_COUNT,
            MAX_DATASTORE_KEY_LENGTH,
            LEDGER_PART_SIZE_MESSAGE_BYTES,
        );
        FinalLedger {
            config: Default::default(),
            sorted_ledger: db,
        }
    }
}
