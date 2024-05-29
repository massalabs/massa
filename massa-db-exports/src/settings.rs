use std::path::PathBuf;

/// Config structure for a `MassaDBRaw`
#[derive(Debug, Clone)]
pub struct MassaDBConfig {
    /// The path to the database, used in the wrapped RocksDB instance
    pub path: PathBuf,
    /// Change history to keep (indexed by ChangeID)
    pub max_history_length: usize,
    /// max_new_elements for bootstrap in versioning stream batch
    pub max_versioning_elements_size: usize,
    /// max_new_elements for bootstrap in final_state stream batch
    pub max_final_state_elements_size: usize,
    /// Thread count for slot serialization
    pub thread_count: u8,
    /// Maximum number of ledger backups to keep
    pub max_ledger_backups: u64,
}
