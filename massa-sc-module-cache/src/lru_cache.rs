use massa_execution_exports::ExecutionError;
use massa_hash::Hash;
use massa_models::prehash::BuildHashMapper;
use massa_sc_runtime::RuntimeModule;
use schnellru::{ByLength, LruMap};

use crate::types::ModuleInfo;

/// `LruMap` specialization for `PreHashed` keys
pub type PreHashLruMap<K, V> = LruMap<K, V, ByLength, BuildHashMapper<K>>;

/// RAM stored LRU cache.
/// The LRU caching scheme is to remove the least recently used module when the cache is full.
///
/// It is composed of:
/// * key: raw bytecode (which is hashed on insertion in LruMap)
/// * value.0: corresponding compiled module
/// * value.1: instance initialization cost
pub(crate) struct LRUCache {
    cache: PreHashLruMap<Hash, ModuleInfo>,
}

impl LRUCache {
    /// Create a new `LRUCache` with the given size
    pub fn new(cache_size: u32) -> Self {
        LRUCache {
            cache: LruMap::with_hasher(ByLength::new(cache_size), BuildHashMapper::default()),
        }
    }

    /// If the module is contained in the cache:
    /// * retrieve a copy of it
    /// * move it up in the LRU cache
    pub fn get(&mut self, hash: Hash, limit: u64) -> Result<Option<ModuleInfo>, ExecutionError> {
        if let Some((cached_module, opt_init_cost)) = self.cache.get(&hash) {
            if let Some(init_cost) = opt_init_cost && limit < *init_cost {
                return Err(ExecutionError::RuntimeError(
                    "given gas cannot cover the initialization costs".to_string(),
                ));
            }
            Ok(Some((cached_module.clone(), *opt_init_cost)))
        } else {
            Ok(None)
        }
    }

    /// Save a module in the LRU cache
    pub fn insert(&mut self, hash: Hash, module_info: ModuleInfo) {
        self.cache.insert(hash, module_info);
    }

    /// Set the initialization cost of a LRU cached module
    pub fn set_init_cost(&mut self, hash: Hash, init_cost: u64) -> Result<(), ExecutionError> {
        if let Some((_, opt_init_cost)) = self.cache.get(&hash) {
            *opt_init_cost = Some(init_cost);
            Ok(())
        } else {
            Err(ExecutionError::RuntimeError(
                "tried to set the init cost of a nonexistent module".to_string(),
            ))
        }
    }
}
