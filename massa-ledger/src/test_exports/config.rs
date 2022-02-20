// Copyright (c) 2022 MASSA LABS <info@massa.net>

/// This file defines testing tools related to the config
use crate::LedgerConfig;

/// Default value of LedgerConfig used for tests
impl Default for LedgerConfig {
    fn default() -> Self {
        LedgerConfig {
            initial_sce_ledger_path: "".into(), // unused by the mock
            final_history_length: 10,
            thread_count: 2,
        }
    }
}
