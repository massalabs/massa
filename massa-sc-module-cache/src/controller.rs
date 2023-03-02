use massa_execution_exports::ExecutionError;
use massa_hash::Hash;
use massa_models::prehash::BuildHashMapper;
use massa_sc_runtime::{GasCosts, RuntimeModule};
use schnellru::{ByLength, LruMap};

use crate::{hd_cache::HDCache, lru_cache::LRUCache, types::ModuleInfo};

/// `LruMap` specialization for `PreHashed` keys
pub type PreHashLruMap<K, V> = LruMap<K, V, ByLength, BuildHashMapper<K>>;

/// Cache controller of compiled runtime modules
pub struct ModuleCache {
    /// Gas costs used to:
    /// * setup `massa-sc-runtime` metering on compilation
    /// * TODO: debit compilation costs
    gas_costs: GasCosts,
    /// RAM stored LRU cache.
    /// See `LRUCache` documentation for more information.
    lru_cache: LRUCache,
    /// Disk stored cache.
    /// See the `HDCache` documentation for more information.
    hd_cache: HDCache,
}

impl ModuleCache {
    pub fn new(gas_costs: GasCosts, lru_cache_size: u32) -> Self {
        Self {
            gas_costs,
            lru_cache: LRUCache::new(lru_cache_size),
            hd_cache: HDCache::new(),
        }
    }

    /// Save a new or an already existing module in the cache
    pub fn save_module(&mut self, bytecode: &[u8], limit: u64) -> Result<(), ExecutionError> {
        // TODO: using ExecutionError for now but create a CacheError type
        let hash = Hash::compute_from(bytecode);
        if let Some(hd_module_info) = self.hd_cache.get_and_increment(hash) {
            self.lru_cache.insert(hash, hd_module_info);
        } else {
            if let Some(lru_module_info) = self.lru_cache.get(hash, limit)? {
                self.hd_cache.insert(hash, lru_module_info);
            } else {
                // CL compiler because this will be stored in HD
                let new_module = RuntimeModule::new(bytecode, limit, self.gas_costs.clone(), true)
                    .map_err(|err| {
                        ExecutionError::RuntimeError(format!(
                            "compilation of missing cache module failed: {}",
                            err
                        ))
                    })?;
                let new_module_info = (new_module, None);
                self.hd_cache.insert(hash, new_module_info.clone());
                self.lru_cache.insert(hash, new_module_info);
            }
        }
        Ok(())
    }

    /// Set the initialization cost of a cached module
    pub fn set_init_cost(
        &mut self,
        bytecode: &[u8],
        init_cost: u64,
        cache_compatible: bool,
    ) -> Result<(), ExecutionError> {
        let hash = Hash::compute_from(bytecode);
        self.lru_cache.set_init_cost(hash, init_cost)?;
        if cache_compatible {
            self.hd_cache.set_init_cost(hash, init_cost)?;
        }
        Ok(())
    }

    pub fn update_module(
        &mut self,
        new: &[u8],
        new_limit: u64,
        previous: &[u8],
    ) -> Result<(), ExecutionError> {
        self.remove_module(previous);
        self.save_module(new, new_limit)?;
        Ok(())
    }

    /// Remove a cached module
    pub fn remove_module(&mut self, bytecode: &[u8]) {
        let hash = Hash::compute_from(bytecode);
        self.hd_cache.remove(hash);
    }

    /// Load a cached module for execution
    pub fn load_module(
        &mut self,
        bytecode: &[u8],
        limit: u64,
    ) -> Result<RuntimeModule, ExecutionError> {
        let hash = Hash::compute_from(bytecode);
        // IMPORTANT TODO: make a gas check in the HD cache as well
        if let Some(hd_module_info) = self.hd_cache.get(hash) {
            if let Some((lru_module, _)) = self.lru_cache.get(hash, limit)? {
                Ok(lru_module)
            } else {
                self.lru_cache.insert(hash, hd_module_info.clone());
                Ok(hd_module_info.0)
            }
        } else {
            if let Some((lru_module, _)) = self.lru_cache.get(hash, limit)? {
                Ok(lru_module)
            } else {
                // SP compiler because this is arbitrary bytecode
                let new_module = RuntimeModule::new(bytecode, limit, self.gas_costs.clone(), false)
                    .map_err(|err| {
                        ExecutionError::RuntimeError(format!(
                            "compilation of missing cache module failed: {}",
                            err
                        ))
                    })?;
                Ok(new_module)
            }
        }
    }
}
