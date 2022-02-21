use massa_models::constants::{GENESIS_TIMESTAMP, T0, THREAD_COUNT};
use massa_time::MassaTime;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Execution setting parsed with .toml in `massa-node/src/settings.rs`
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ExecutionSettings {
    /// Initial SCE ledger file
    pub initial_sce_ledger_path: PathBuf,
    /// maximum number of SC output events kept in cache
    pub max_final_events: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ExecutionConfigs {
    /// Execution settings
    pub settings: ExecutionSettings,
    /// Thread count
    pub thread_count: u8,
    /// Genesis timestmap
    pub genesis_timestamp: MassaTime,
    /// period duration
    pub t0: MassaTime,
    /// clock compensation in milliseconds
    pub clock_compensation: i64,
}

impl Default for ExecutionConfigs {
    fn default() -> Self {
        Self {
            settings: Default::default(),
            thread_count: THREAD_COUNT,
            genesis_timestamp: *GENESIS_TIMESTAMP,
            t0: T0,
            clock_compensation: Default::default(),
        }
    }
}
