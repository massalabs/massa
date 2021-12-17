use massa_time::MassaTime;
use serde::{Deserialize, Serialize};

/// Max size for channels used with communication with other components.
pub const CHANNEL_SIZE: usize = 256;

/// Execution configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ExecutionConfig {
    pub thread_count: u8,
    pub genesis_timestamp: MassaTime,
    pub t0: MassaTime,
    pub clock_compensation: i64,
}
