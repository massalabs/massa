// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::sync::Arc;

use massa_db_exports::MassaDBConfig;
use massa_db_worker::MassaDB;
use parking_lot::RwLock;
/// This file defines testing tools related to the configuration
use tempfile::TempDir;

use crate::{ledger_db::LedgerDB, FinalLedger};
use massa_models::config::{MAX_DATASTORE_KEY_LENGTH, MAX_DATASTORE_VALUE_LENGTH, THREAD_COUNT};

/// Default value of `FinalLedger` used for tests
impl Default for FinalLedger {
    fn default() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 10,
            max_new_elements: 100,
            thread_count: THREAD_COUNT,
        };
        let db = MassaDB::new(db_config);
        let db = LedgerDB::new(
            Arc::new(RwLock::new(Box::new(db))),
            THREAD_COUNT,
            MAX_DATASTORE_KEY_LENGTH,
            MAX_DATASTORE_VALUE_LENGTH,
        );
        FinalLedger {
            config: Default::default(),
            sorted_ledger: db,
        }
    }
}
