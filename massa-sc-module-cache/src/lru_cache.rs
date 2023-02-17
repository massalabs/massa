use massa_execution_exports::ExecutionError;
use massa_hash::Hash;
use massa_models::prehash::BuildHashMapper;
use massa_sc_runtime::RuntimeModule;
use schnellru::{ByLength, LruMap};

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
    cache: PreHashLruMap<Hash, (RuntimeModule, u64)>,
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
    pub fn get(&mut self, hash: Hash, limit: u64) -> Result<Option<RuntimeModule>, ExecutionError> {
        if let Some((cached_module, init_cost)) = self.cache.get(&hash) {
            if limit < *init_cost {
                return Err(ExecutionError::RuntimeError(
                    "given gas cannot cover the initialization costs".to_string(),
                ));
            }
            Ok(Some(cached_module.clone()))
        } else {
            Ok(None)
        }
    }

    /// Save a module in the LRU cache
    pub fn insert(&mut self, hash: Hash, module: RuntimeModule, init_cost: u64) {
        self.cache.insert(hash, (module, init_cost));
    }
}
