use serde::{Deserialize, Serialize};

/// Max size for channels used with communication with other components.
pub const CHANNEL_SIZE: usize = 256;

/// Execution configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ExecutionConfig {
    pub max_gas_per_block: u64,
}
