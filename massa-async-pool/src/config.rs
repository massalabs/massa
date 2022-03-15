//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a config strucutre containing all settings for the async message pool system

/// Ledger configuration
#[derive(Debug, Clone)]
pub struct AsyncPoolConfig {
    /// max number of messages in the pool
    pub initial_sce_ledger_path: PathBuf,

}
