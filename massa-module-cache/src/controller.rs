use massa_hash::Hash;
use massa_models::prehash::BuildHashMapper;
use massa_sc_runtime::{GasCosts, RuntimeModule};
use schnellru::{ByLength, LruMap};

use crate::{error::CacheError, hd_cache::HDCache, lru_cache::LRUCache, types::ModuleInfo};

/// `LruMap` specialization for `PreHashed` keys
pub type PreHashLruMap<K, V> = LruMap<K, V, ByLength, BuildHashMapper<K>>;

/// Cache controller of compiled runtime modules
pub struct ModuleCache {
    /// Gas costs used to:
    /// * setup `massa-sc-runtime` metering on compilation
    /// * TODO: debit compilation costs
    gas_costs: GasCosts,
    /// Default gas for compilation
    compilation_gas: u64,
    /// RAM stored LRU cache.
    /// See `LRUCache` documentation for more information.
    lru_cache: LRUCache,
    /// Disk stored cache.
    /// See the `HDCache` documentation for more information.
    hd_cache: HDCache,
}

impl ModuleCache {
    pub fn new(gas_costs: GasCosts, lru_cache_size: u32, compilation_gas: u64) -> Self {
        Self {
            gas_costs,
            compilation_gas,
            lru_cache: LRUCache::new(lru_cache_size),
            hd_cache: HDCache::new("hd_cache_path".into(), 1000, 10),
        }
    }

    /// Save a new or an already existing module in the cache
    pub fn save_module(&mut self, bytecode: &[u8]) -> Result<(), CacheError> {
        let hash = Hash::compute_from(bytecode);
        if let Some(hd_module_info) =
            self.hd_cache
                .get(hash, self.compilation_gas, self.gas_costs.clone())
        {
            self.lru_cache.insert(hash, hd_module_info);
        } else {
            if let Some(lru_module_info) = self.lru_cache.get(hash) {
                self.hd_cache.insert(hash, lru_module_info)?;
            } else {
                let new_module = RuntimeModule::new(
                    bytecode,
                    self.compilation_gas,
                    self.gas_costs.clone(),
                    true,
                )?;
                let new_module_info = ModuleInfo::Module(new_module);
                self.hd_cache.insert(hash, new_module_info.clone())?;
                self.lru_cache.insert(hash, new_module_info);
            }
        }
        Ok(())
    }

    /// Set the initialization cost of a cached module
    pub fn set_init_cost(&mut self, bytecode: &[u8], init_cost: u64) {
        let hash = Hash::compute_from(bytecode);
        self.lru_cache.set_init_cost(hash, init_cost);
        self.hd_cache.set_init_cost(hash, init_cost);
    }

    pub fn set_invalid(&mut self, bytecode: &[u8]) {
        let hash = Hash::compute_from(bytecode);
        self.lru_cache.set_invalid(hash);
        self.hd_cache.set_invalid(hash);
    }

    /// Load a cached module for execution
    fn load_module_info(&mut self, bytecode: &[u8], limit: u64) -> Result<ModuleInfo, CacheError> {
        // TODO: check if limit can be removed here
        let hash = Hash::compute_from(bytecode);
        if let Some(lru_module_info) = self.lru_cache.get(hash) {
            Ok(lru_module_info)
        } else {
            if let Some(hd_module_info) = self.hd_cache.get(hash, limit, self.gas_costs.clone()) {
                self.lru_cache.insert(hash, hd_module_info.clone());
                Ok(hd_module_info)
            } else {
                let new_module = RuntimeModule::new(bytecode, limit, self.gas_costs.clone(), true)?;
                let new_module_info = ModuleInfo::Module(new_module);
                // TODO: move this back out
                // TODO: maybe keep it but set as invalid if compil failed
                self.hd_cache.insert(hash, new_module_info.clone())?;
                self.lru_cache.insert(hash, new_module_info.clone());
                Ok(new_module_info)
            }
        }
    }

    /// Load a cached module for execution and check its validity for execution
    pub fn load_module(
        &mut self,
        bytecode: &[u8],
        limit: u64,
    ) -> Result<RuntimeModule, CacheError> {
        let module_info = self.load_module_info(&bytecode, limit)?;
        let module = match module_info {
            ModuleInfo::Invalid => {
                return Err(CacheError::LoadError("Loading invalid module".to_string()));
            }
            ModuleInfo::Module(module) => module,
            ModuleInfo::ModuleAndDelta((module, delta)) => {
                if delta > limit {
                    return Err(CacheError::LoadError(
                        "Provided max gas is below the instance creation cost".to_string(),
                    ));
                } else {
                    module
                }
            }
        };
        Ok(module)
    }

    /// Load a temporary module from arbitrary bytecode
    pub fn load_tmp_module(
        &self,
        bytecode: &[u8],
        limit: u64,
    ) -> Result<RuntimeModule, CacheError> {
        Ok(RuntimeModule::new(
            bytecode,
            limit,
            self.gas_costs.clone(),
            true,
        )?)
    }
}
