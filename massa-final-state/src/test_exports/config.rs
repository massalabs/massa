//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines testing tools related to the configuration

use crate::FinalStateConfig;
use massa_async_pool::AsyncPoolConfig;
use massa_ledger::LedgerConfig;
use tempdir::TempDir;

/// Default value of `FinalStateConfig` used for tests
impl Default for FinalStateConfig {
    fn default() -> Self {
        FinalStateConfig {
            ledger_config: LedgerConfig {
                initial_sce_ledger_path: Default::default(),
                disk_ledger_path: TempDir::new("disk_ledger").unwrap().path().to_path_buf(),
            },
            async_pool_config: AsyncPoolConfig::default(),
            final_history_length: 10,
            thread_count: 2,
        }
    }
}
