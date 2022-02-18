// Copyright (c) 2021 MASSA LABS <info@massa.net>

use std::path::PathBuf;

/// Ledger configuration
#[derive(Debug, Clone)]
pub struct LedgerConfig {
    /// initial SCE ledger file
    pub initial_sce_ledger_path: PathBuf,
    /// final changes history length
    pub final_history_length: usize,
    /// thread count
    pub thread_count: u8,
}
