// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a configuration structure containing all settings for the ledger system

use std::path::PathBuf;

/// Ledger configuration
#[derive(Debug, Clone)]
pub struct LedgerConfig {
    /// initial SCE ledger file
    pub initial_sce_ledger_path: PathBuf,
    /// disk ledger db directory
    pub disk_ledger_path: PathBuf,
    /// max key length
    pub max_key_length: u8,
    /// max ledger part size
    pub max_ledger_part_size: u64,
    /// address bytes size
    pub address_bytes_size: usize,
}
