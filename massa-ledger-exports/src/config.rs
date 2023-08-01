// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a configuration structure containing all settings for the ledger system

use std::path::PathBuf;

/// Ledger configuration
#[derive(Debug, Clone)]
pub struct LedgerConfig {
    /// thread count
    pub thread_count: u8,
    /// initial SCE ledger file
    pub initial_ledger_path: PathBuf,
    /// disk ledger db directory
    pub disk_ledger_path: PathBuf,
    /// max key length
    pub max_key_length: u8,
    /// max datastore value length
    pub max_datastore_value_length: u64,
}
