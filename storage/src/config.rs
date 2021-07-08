use serde::Deserialize;
pub const CHANNEL_SIZE: usize = 16;
#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    /// Max number of blocks we want to store
    pub max_stored_blocks: usize,
    /// path to db
    pub path: String,
    pub cache_capacity: u64,
    pub flush_every_ms: Option<u64>,
}
