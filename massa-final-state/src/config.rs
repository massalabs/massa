//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a configuration structure containing all settings for final state management

use massa_async_pool::AsyncPoolConfig;
use massa_ledger::LedgerConfig;

/// Ledger configuration
#[derive(Debug, Clone)]
pub struct FinalStateConfig {
    /// ledger configuration
    pub ledger_config: LedgerConfig,
    /// asynchronous pool configuration
    pub async_pool_config: AsyncPoolConfig,
    /// final changes history length
    pub final_history_length: usize,
    /// thread count
    pub thread_count: u8,
}
