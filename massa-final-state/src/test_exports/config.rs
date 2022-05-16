//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines testing tools related to the configuration

use crate::FinalStateConfig;
use massa_async_pool::AsyncPoolConfig;
use massa_ledger::LedgerConfig;

/// Default value of `FinalStateConfig` used for tests
impl Default for FinalStateConfig {
    fn default() -> Self {
        FinalStateConfig {
            ledger_config: LedgerConfig::default(),
            async_pool_config: AsyncPoolConfig::default(),
            final_history_length: 10,
            thread_count: 2,
        }
    }
}
