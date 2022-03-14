//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a config structure containing all settings for the execution state system

use massa_ledger::LedgerConfig;

/// Ledger configuration
#[derive(Debug, Clone)]
pub struct FinalStateConfig {
    /// ledger config
    pub ledger_config: LedgerConfig,
    /// final changes history length
    pub final_history_length: usize,
    /// thread count
    pub thread_count: u8,
}
