//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines testing tools related to the configuration

use std::collections::BTreeMap;

use crate::FinalStateConfig;
use massa_async_pool::AsyncPoolConfig;
use massa_ledger::LedgerConfig;
use tempfile::{NamedTempFile, TempDir};

impl FinalStateConfig {
    /// Get sample final state configuration
    pub fn sample() -> (Self, NamedTempFile, TempDir) {
        let (ledger_config, keep_file, keep_dir) = LedgerConfig::sample(&BTreeMap::new());
        (
            FinalStateConfig {
                ledger_config,
                async_pool_config: AsyncPoolConfig::default(),
                final_history_length: 10,
                thread_count: 2,
            },
            keep_file,
            keep_dir,
        )
    }
}
