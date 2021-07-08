mod block_storage;
mod config;
mod error;
mod storage_access;

pub use config::StorageConfig;
pub use error::StorageError;
pub use storage_access::{start_storage, StorageAccess, StorageManager};

#[cfg(test)]
mod tests;
