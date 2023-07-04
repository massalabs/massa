use std::path::PathBuf;

/// Config structure for a `MassaDBRaw`
#[derive(Debug, Clone)]
pub struct MassaDBConfig {
    /// The path to the database, used in the wrapped RocksDB instance
    pub path: PathBuf,
    /// Change history to keep (indexed by ChangeID)
    pub max_history_length: usize,
    /// max_new_elements for bootstrap
    pub max_new_elements: usize,
    /// Thread count for slot serialization
    pub thread_count: u8,
}
