// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a configuration structure containing all settings for the ledger system

use std::path::PathBuf;

/// Ledger configuration
#[derive(Debug, Clone)]
pub struct LedgerConfig {
    /// initial SCE ledger file
    pub initial_sce_ledger_path: PathBuf,
}
