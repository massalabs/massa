use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Max size for channels used with communication with other components.
pub const CHANNEL_SIZE: usize = 256;

/// Execution configuration
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ExecutionSettings {
    /// Initial SCE ledger file
    pub initial_sce_ledger_path: PathBuf,
}
