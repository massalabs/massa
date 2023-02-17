use massa_hash::Hash;
use massa_sc_runtime::RuntimeModule;

pub struct HDCache {}

impl HDCache {
    pub fn new() -> Self {
        Self {}
    }

    pub fn add(&self, bc_hash: Hash) {}

    pub fn get(&self, bc_hash: Hash) -> Option<RuntimeModule> {
        None
    }

    pub fn delete(&self, bc_hash: Hash) {}
}
