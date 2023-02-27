use massa_hash::Hash;
use massa_sc_runtime::RuntimeModule;

pub(crate) struct HDCache {}

impl HDCache {
    pub fn new() -> Self {
        Self {}
    }

    pub fn insert(&self, hash: Hash, module: RuntimeModule, opt_init_cost: Option<u64>) {}

    pub fn set_init_cost(&self, hash: Hash, init_cost: u64) {}

    pub fn get_and_increment(&self, hash: Hash) -> Option<(RuntimeModule, u64)> {
        None
    }

    pub fn get(&self, hash: Hash) -> Option<(RuntimeModule, u64)> {
        None
    }

    pub fn remove(&self, hash: Hash) {}
}
