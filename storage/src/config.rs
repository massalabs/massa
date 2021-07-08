use serde::Deserialize;
pub const CHANNEL_SIZE: usize = 16;
#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    /// Max number of bytes we want to store
    max_capacity: u64,
}
