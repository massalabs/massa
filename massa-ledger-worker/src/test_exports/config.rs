// Copyright (c) 2022 MASSA LABS <info@massa.net>

/// This file defines testing tools related to the configuration
use tempfile::TempDir;

use crate::{ledger_db::LedgerDB, FinalLedger};
use massa_models::constants::default_testing::ADDRESS_SIZE_BYTES;

/// Default value of `FinalLedger` used for tests
impl Default for FinalLedger {
    fn default() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let db = LedgerDB::new(temp_dir.path().to_path_buf(), 32, 10000, ADDRESS_SIZE_BYTES);
        FinalLedger {
            _config: Default::default(),
            sorted_ledger: db,
        }
    }
}
