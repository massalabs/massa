use serde::Deserialize;
pub const CHANNEL_SIZE: usize = 16;
#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    /// Max number of bytes we want to store
    pub max_capacity: u64,
    /// path to db
    pub path: String,
    pub cache_capacity: u64,
    pub flush_every_ms: Option<u64>,
}
