use massa_execution_exports::ExecutionError;
use massa_sc_runtime::{GasCosts, RuntimeModule};
use schnellru::{ByLength, LruMap};
use tracing::log::warn;

pub struct ModuleCache {
    gas_costs: GasCosts,
    cache: LruMap<Vec<u8>, RuntimeModule>,
}

impl ModuleCache {
    pub fn new(gas_costs: GasCosts, cache_size: u32) -> Self {
        Self {
            gas_costs,
            cache: LruMap::new(ByLength::new(cache_size)),
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
        let module = if let Some(cached_module) = self.cache.get(bytecode) {
            warn!("(CACHE) found");
            cached_module.clone()
        } else {
            warn!("(CACHE) compiled");
            let new_module =
                RuntimeModule::new(bytecode, limit, self.gas_costs.clone()).map_err(|err| {
                    ExecutionError::RuntimeError(format!(
                        "compilation of missing cache module failed: {}",
                        err
                    ))
                })?;
            new_module
        };
        Ok(module)
    }

    /// Save a module in the cache
    pub fn save_module(&mut self, bytecode: &[u8], module: RuntimeModule) {
        warn!("(CACHE) saved");
        self.cache.insert(bytecode.to_vec(), module);
    }
}
