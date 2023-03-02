use massa_execution_exports::ExecutionError;
use massa_hash::Hash;

use crate::types::ModuleInfo;

pub(crate) struct HDCache {}

impl HDCache {
    /// Create a new HDCache
    pub fn new() -> Self {
        Self {}
    }

    /// Insert a new module in the cache
    pub fn insert(&self, hash: Hash, module_info: ModuleInfo) {}

    /// Sets the initialization cost of a given module separately
    pub fn set_init_cost(&self, hash: Hash, init_cost: u64) -> Result<(), ExecutionError> {
        Ok(())
    }

    /// Retrieve a module and increment its associated reference counter
    pub fn get_and_increment(&self, hash: Hash) -> Option<ModuleInfo> {
        None
    }

    /// Retrieve a module
    pub fn get(&self, hash: Hash) -> Option<ModuleInfo> {
        None
    }

    /// Remove a module
    pub fn remove(&self, hash: Hash) {}
}
