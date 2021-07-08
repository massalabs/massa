use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    /// Max number of bytes we want to store
    max_capacity: u64
}