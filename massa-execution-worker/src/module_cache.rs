use massa_execution_exports::ExecutionError;
use massa_hash::Hash;
use massa_models::prehash::BuildHashMapper;
use massa_sc_runtime::{GasCosts, RuntimeModule};
use schnellru::{ByLength, LruMap};

/// `LruMap` specialization for `PreHashed` keys
pub type PreHashLruMap<K, V> = LruMap<K, V, ByLength, BuildHashMapper<K>>;

/// LRU cache of compiled runtime modules.
/// The LRU caching scheme is to remove the least recently used module when the cache is full.
///
/// * key: raw bytecode (which is hashed on insertion in LruMap)
/// * value.0: corresponding compiled module
/// * value.1: instance initialization cost
pub struct ModuleCache {
    gas_costs: GasCosts,
    cache: PreHashLruMap<Hash, (RuntimeModule, u64)>,
}

impl ModuleCache {
    pub fn new(gas_costs: GasCosts, cache_size: u32) -> Self {
        Self {
            gas_costs,
            cache: LruMap::with_hasher(ByLength::new(cache_size), BuildHashMapper::default()),
        }
    }

    /// If the module is contained in the cache:
    /// * retrieve a copy of it
    /// * move it up in the LRU cache
    ///
    /// If the module is not contained in the cache:
    /// * create the module
    /// * retrieve it
    pub fn get_module(
        &mut self,
        bytecode: &[u8],
        limit: u64,
    ) -> Result<RuntimeModule, ExecutionError> {
        if let Some((cached_module, init_cost)) = self.cache.get(&Hash::compute_from(bytecode)) {
            if limit < *init_cost {
                return Err(ExecutionError::RuntimeError(
                    "given gas cannot cover the initialization costs".to_string(),
                ));
            }
            Ok(cached_module.clone())
        } else {
            let new_module =
                RuntimeModule::new(bytecode, limit, self.gas_costs.clone()).map_err(|err| {
                    ExecutionError::RuntimeError(format!(
                        "compilation of missing cache module failed: {}",
                        err
                    ))
                })?;
            Ok(new_module)
        }
    }

    /// Save a module in the cache
    pub fn save_module(&mut self, bytecode: &[u8], module: RuntimeModule, init_cost: u64) {
        self.cache
            .insert(Hash::compute_from(bytecode), (module, init_cost));
    }
}
