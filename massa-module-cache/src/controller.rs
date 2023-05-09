use massa_hash::Hash;
use massa_models::prehash::BuildHashMapper;
use massa_sc_runtime::{Compiler, RuntimeModule};
use schnellru::{ByLength, LruMap};
use tracing::{debug, warn};

use crate::{
    config::ModuleCacheConfig, error::CacheError, hd_cache::HDCache, lru_cache::LRUCache,
    types::ModuleInfo,
};

/// `LruMap` specialization for `PreHashed` keys
pub(crate) type PreHashLruMap<K, V> = LruMap<K, V, ByLength, BuildHashMapper<K>>;

/// Cache controller of compiled runtime modules
pub struct ModuleCache {
    /// Cache config.
    /// See `CacheConfig` documentation for more information.
    cfg: ModuleCacheConfig,
    /// RAM stored LRU cache.
    /// See `LRUCache` documentation for more information.
    lru_cache: LRUCache,
    /// Disk stored cache.
    /// See the `HDCache` documentation for more information.
    hd_cache: HDCache,
}

impl ModuleCache {
    /// Creates a new `ModuleCache`
    pub fn new(cfg: ModuleCacheConfig) -> Self {
        Self {
            lru_cache: LRUCache::new(cfg.lru_cache_size),
            hd_cache: HDCache::new(
                cfg.hd_cache_path.clone(),
                cfg.hd_cache_size,
                cfg.snip_amount,
            ),
            cfg,
        }
    }

    /// Internal function to compile and build `ModuleInfo`
    fn compile_cached(&mut self, bytecode: &[u8], hash: Hash) -> ModuleInfo {
        match RuntimeModule::new(
            bytecode,
            self.cfg.compilation_gas,
            self.cfg.gas_costs.clone(),
            Compiler::CL,
        ) {
            Ok(module) => {
                debug!("compilation of module {} succeeded", hash);
                ModuleInfo::Module(module)
            }
            Err(e) => {
                warn!("compilation of module {} failed with: {}", hash, e);
                ModuleInfo::Invalid
            }
        }
    }

    /// Save a new or an already existing module in the cache
    pub fn save_module(&mut self, bytecode: &[u8]) {
        let hash = Hash::compute_from(bytecode);
        if let Some(hd_module_info) =
            self.hd_cache
                .get(hash, self.cfg.compilation_gas, self.cfg.gas_costs.clone())
        {
            debug!("save_module: {} present in hd", hash);
            self.lru_cache.insert(hash, hd_module_info);
        } else if let Some(lru_module_info) = self.lru_cache.get(hash) {
            debug!("save_module: {} missing in hd but present in lru", hash);
            self.hd_cache.insert(hash, lru_module_info);
        } else {
            debug!("save_module: {} missing", hash);
            let module_info = self.compile_cached(bytecode, hash);
            self.hd_cache.insert(hash, module_info.clone());
            self.lru_cache.insert(hash, module_info);
        }
    }

    /// Set the initialization cost of a cached module
    pub(crate) fn set_init_cost(&mut self, bytecode: &[u8], init_cost: u64) {
        let hash = Hash::compute_from(bytecode);
        self.lru_cache.set_init_cost(hash, init_cost);
        self.hd_cache.set_init_cost(hash, init_cost);
    }

    /// Set a cached module as invalid
    pub(crate) fn set_invalid(&mut self, bytecode: &[u8]) {
        let hash = Hash::compute_from(bytecode);
        self.lru_cache.set_invalid(hash);
        self.hd_cache.set_invalid(hash);
    }

    /// Load a cached module for execution
    fn load_module_info(&mut self, bytecode: &[u8]) -> ModuleInfo {
        let hash = Hash::compute_from(bytecode);
        if let Some(lru_module_info) = self.lru_cache.get(hash) {
            debug!("load_module: {} present in lru", hash);
            lru_module_info
        } else if let Some(hd_module_info) =
            self.hd_cache
                .get(hash, self.cfg.compilation_gas, self.cfg.gas_costs.clone())
        {
            debug!("load_module: {} missing in lru but present in hd", hash);
            self.lru_cache.insert(hash, hd_module_info.clone());
            hd_module_info
        } else {
            debug!("load_module: {} missing", hash);
            let module_info = self.compile_cached(bytecode, hash);
            self.hd_cache.insert(hash, module_info.clone());
            self.lru_cache.insert(hash, module_info.clone());
            module_info
        }
    }

    /// Load a cached module for execution and check its validity for execution
    pub(crate) fn load_module(
        &mut self,
        bytecode: &[u8],
        execution_gas: u64,
    ) -> Result<RuntimeModule, CacheError> {
        let module_info = self.load_module_info(bytecode);
        let module = match module_info {
            ModuleInfo::Invalid => {
                return Err(CacheError::LoadError("Loading invalid module".to_string()));
            }
            ModuleInfo::Module(module) => module,
            ModuleInfo::ModuleAndDelta((module, delta)) => {
                if delta > execution_gas {
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
        debug!("load_tmp_module");
        Ok(RuntimeModule::new(
            bytecode,
            limit,
            self.cfg.gas_costs.clone(),
            Compiler::SP,
        )?)
    }
}
