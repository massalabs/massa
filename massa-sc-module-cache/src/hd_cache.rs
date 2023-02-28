use massa_hash::Hash;
use massa_sc_runtime::RuntimeModule;

pub(crate) struct HDCache {}

impl HDCache {
    /// Create a new HDCache
    pub fn new() -> Self {
        Self {}
    }

    /// Insert a new module in the cache
    pub fn insert(&self, hash: Hash, module: RuntimeModule, opt_init_cost: Option<u64>) {}

    /// Sets the initialization cost of a given module separately
    pub fn set_init_cost(&self, hash: Hash, init_cost: u64) -> Result<(), ExecutionError> {
        Ok(())
    }

    /// Retrieve a module and increment its associated reference counter
    pub fn get_and_increment(&self, hash: Hash) -> Option<(RuntimeModule, u64)> {
        None
    }

    /// Retrieve a module
    pub fn get(&self, hash: Hash) -> Option<(RuntimeModule, u64)> {
        None
    }

    /// Remove a module
    pub fn remove(&self, hash: Hash) {}
}
