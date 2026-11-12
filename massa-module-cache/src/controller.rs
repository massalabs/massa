use massa_hash::Hash;
use massa_models::prehash::BuildHashMapper;
use massa_sc_runtime::{Compiler, CondomLimits, RuntimeModule};
use schnellru::{ByLength, LruMap};
use tracing::debug;

use crate::{
    config::ModuleCacheConfig, error::CacheError, hd_cache::HDCache, lru_cache::LRUCache,
    types::ModuleInfo,
};

/// `LruMap` specialization for `PreHashed` keys
pub type PreHashLruMap<K, V> = LruMap<K, V, ByLength, BuildHashMapper<K>>;

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

    pub fn reset(&mut self) {
        self.lru_cache.reset();
        self.hd_cache.reset();
    }

    /// Internal function to compile and build `ModuleInfo`
    fn compile_cached(
        &mut self,
        bytecode: &[u8],
        hash: Hash,
        condom_limits: CondomLimits,
    ) -> ModuleInfo {
        match RuntimeModule::new(
            bytecode,
            self.cfg.gas_costs.clone(),
            Compiler::CL,
            condom_limits,
        ) {
            Ok(module) => {
                debug!("compilation of module {} succeeded", hash);
                ModuleInfo::Module(module)
            }
            Err(e) => {
                let err_msg = format!("compilation of module {} failed: {}", hash, e);
                debug!(err_msg);
                ModuleInfo::Invalid(err_msg)
            }
        }
    }

    /// Save a new or an already existing module in the cache
    pub fn save_module(&mut self, bytecode: &[u8], condom_limits: CondomLimits) {
        let hash = Hash::compute_from(bytecode);
        if let Some(lru_module_info) = self.lru_cache.get(hash) {
            debug!("save_module: {} present in lru", hash);
            self.hd_cache.insert(hash, lru_module_info);
        } else if let Some(hd_module_info) =
            self.hd_cache
                .get(hash, self.cfg.gas_costs.clone(), condom_limits.clone())
        {
            debug!("save_module: {} missing in lru but present in hd", hash);
            self.lru_cache.insert(hash, hd_module_info);
        } else {
            debug!("save_module: {} missing", hash);
            let module_info = self.compile_cached(bytecode, hash, condom_limits);
            self.hd_cache.insert(hash, module_info.clone());
            self.lru_cache.insert(hash, module_info);
        }
    }

    /// Set the initialization cost of a cached module
    pub fn set_init_cost(&mut self, bytecode: &[u8], init_cost: u64) {
        let hash = Hash::compute_from(bytecode);
        self.lru_cache.set_init_cost(hash, init_cost);
        self.hd_cache.set_init_cost(hash, init_cost);
    }

    /// Set a cached module as invalid
    pub fn set_invalid(&mut self, bytecode: &[u8], err_msg: String) {
        let hash = Hash::compute_from(bytecode);
        self.lru_cache.set_invalid(hash, err_msg.clone());
        self.hd_cache.set_invalid(hash, err_msg);
    }

    /// Load a cached module for execution
    ///
    /// Returns the module information, it can be:
    /// * `ModuleInfo::Invalid` if the module is invalid
    /// * `ModuleInfo::Module` if the module is valid and has no delta
    /// * `ModuleInfo::ModuleAndDelta` if the module is valid and has a delta
    fn load_module_info(&mut self, bytecode: &[u8], condom_limits: CondomLimits) -> ModuleInfo {
        if let Some(invalid) = self.check_module_length(bytecode.len()) {
            return invalid;
        }
        let hash = Hash::compute_from(bytecode);
        self.load_module_info_with_hash(bytecode, hash, condom_limits)
    }

    /// Length checks of `load_module_info`, done before hashing the bytecode
    fn check_module_length(&self, bytecode_len: usize) -> Option<ModuleInfo> {
        if bytecode_len == 0 {
            let error_msg = "load_module: bytecode is absent".to_string();
            debug!(error_msg);
            return Some(ModuleInfo::Invalid(error_msg));
        }
        if bytecode_len > self.cfg.max_module_length as usize {
            let error_msg = format!(
                "load_module: bytecode length {} exceeds max module length {}",
                bytecode_len, self.cfg.max_module_length
            );
            debug!(error_msg);
            return Some(ModuleInfo::Invalid(error_msg));
        }
        None
    }

    /// Looks up the module info of a bytecode whose hash is already known in the
    /// LRU then the HD cache.
    fn get_cached_module_info(
        &mut self,
        hash: Hash,
        condom_limits: CondomLimits,
    ) -> Option<ModuleInfo> {
        if let Some(lru_module_info) = self.lru_cache.get(hash) {
            debug!("load_module: {} present in lru", hash);
            Some(lru_module_info)
        } else if let Some(hd_module_info) =
            self.hd_cache
                .get(hash, self.cfg.gas_costs.clone(), condom_limits)
        {
            debug!("load_module: {} missing in lru but present in hd", hash);
            self.lru_cache.insert(hash, hd_module_info.clone());
            Some(hd_module_info)
        } else {
            None
        }
    }

    /// `load_module_info` after the length checks, with `hash == Hash::compute_from(bytecode)`
    fn load_module_info_with_hash(
        &mut self,
        bytecode: &[u8],
        hash: Hash,
        condom_limits: CondomLimits,
    ) -> ModuleInfo {
        if let Some(module_info) = self.get_cached_module_info(hash, condom_limits.clone()) {
            module_info
        } else {
            debug!("load_module: {} missing", hash);
            let module_info = self.compile_cached(bytecode, hash, condom_limits);
            self.hd_cache.insert(hash, module_info.clone());
            self.lru_cache.insert(hash, module_info.clone());
            module_info
        }
    }

    /// Load a cached module for execution and check its validity for execution.
    /// Also checks that the provided execution gas is enough to pay for the instance creation cost.
    ///
    /// Returns the module after loading.
    pub fn load_module(
        &mut self,
        bytecode: &[u8],
        execution_gas: u64,
        condom_limits: CondomLimits,
    ) -> Result<RuntimeModule, CacheError> {
        self.check_execution_gas(execution_gas)?;
        let module_info = self.load_module_info(bytecode, condom_limits);
        Self::module_from_info(module_info, execution_gas)
    }

    /// Same as `load_module`, when the caller already knows `hash == Hash::compute_from(bytecode)`.
    pub fn load_module_with_hash(
        &mut self,
        bytecode: &[u8],
        hash: Hash,
        execution_gas: u64,
        condom_limits: CondomLimits,
    ) -> Result<RuntimeModule, CacheError> {
        debug_assert_eq!(hash, Hash::compute_from(bytecode));
        self.check_execution_gas(execution_gas)?;
        let module_info = match self.check_module_length(bytecode.len()) {
            Some(invalid) => invalid,
            None => self.load_module_info_with_hash(bytecode, hash, condom_limits),
        };
        Self::module_from_info(module_info, execution_gas)
    }

    /// Same as `load_module` for a bytecode known only by its hash and length,
    /// without access to the bytecode itself.
    ///
    /// Returns `None` when the result cannot be decided without the bytecode
    /// (module in neither the LRU nor the HD cache, or invalid length): the
    /// caller must then fall back to `load_module_with_hash`. Otherwise, returns
    /// exactly what `load_module` would for that bytecode.
    pub fn load_cached_module_by_hash(
        &mut self,
        hash: Hash,
        bytecode_len: usize,
        execution_gas: u64,
        condom_limits: CondomLimits,
    ) -> Option<Result<RuntimeModule, CacheError>> {
        if let Err(err) = self.check_execution_gas(execution_gas) {
            return Some(Err(err));
        }
        if self.check_module_length(bytecode_len).is_some() {
            return None;
        }
        let module_info = self.get_cached_module_info(hash, condom_limits)?;
        Some(Self::module_from_info(module_info, execution_gas))
    }

    /// Gas check done by `load_module` before loading anything
    fn check_execution_gas(&self, execution_gas: u64) -> Result<(), CacheError> {
        // Do not actually debit the instance creation cost from the provided gas
        // This is only supposed to be a check
        execution_gas
            .checked_sub(self.cfg.gas_costs.max_instance_cost)
            .ok_or(CacheError::LoadError(format!(
                "Provided gas {} is lower than the base instance creation gas cost {}",
                execution_gas, self.cfg.gas_costs.max_instance_cost
            )))?;
        // TODO: interesting but unimportant optim
        // remove max_instance_cost hard check if module is cached and has a delta
        Ok(())
    }

    /// Turns the loaded module info into a module, checking the instance creation cost
    fn module_from_info(
        module_info: ModuleInfo,
        execution_gas: u64,
    ) -> Result<RuntimeModule, CacheError> {
        let module = match module_info {
            ModuleInfo::Invalid(err) => {
                let err_msg = format!("invalid module: {}", err);
                return Err(CacheError::LoadError(err_msg));
            }
            ModuleInfo::Module(module) => module,
            ModuleInfo::ModuleAndDelta((module, delta)) => {
                if delta > execution_gas {
                    return Err(CacheError::LoadError(format!(
                        "Provided gas {} is below the gas cost of instance creation ({})",
                        execution_gas, delta
                    )));
                } else {
                    module
                }
            }
        };
        Ok(module)
    }

    /// Load a temporary module from arbitrary bytecode.
    /// Also checks that the provided execution gas is enough to pay for the instance creation cost.
    ///
    /// Returns the module after compilation.
    pub fn load_tmp_module(
        &self,
        bytecode: &[u8],
        limit: u64,
        condom_limits: CondomLimits,
    ) -> Result<RuntimeModule, CacheError> {
        debug!("load_tmp_module");
        if bytecode.is_empty() {
            let error_msg = "load_tmp_module: bytecode is absent".to_string();
            debug!(error_msg);
            return Err(CacheError::LoadError(error_msg));
        }
        if bytecode.len() > self.cfg.max_module_length as usize {
            let error_msg = format!(
                "load_tmp_module: bytecode length {} exceeds max module length {}",
                bytecode.len(),
                self.cfg.max_module_length
            );
            debug!(error_msg);
            return Err(CacheError::LoadError(error_msg));
        }
        // Do not actually debit the instance creation cost from the provided gas
        // This is only supposed to be a check
        limit
            .checked_sub(self.cfg.gas_costs.max_instance_cost)
            .ok_or(CacheError::LoadError(format!(
                "Provided gas {} is lower than the base instance creation gas cost {}",
                limit, self.cfg.gas_costs.max_instance_cost
            )))?;
        let module = RuntimeModule::new(
            bytecode,
            self.cfg.gas_costs.clone(),
            Compiler::SP,
            condom_limits,
        )?;
        Ok(module)
    }

    /// Returns the memory usage of the LRU cache
    pub fn get_module_lru_cache_memory_usage(&self) -> usize {
        self.lru_cache.cache.memory_usage()
    }

    /// Returns the number of modules currently held in the in-RAM LRU cache
    pub fn lru_cache_len(&self) -> usize {
        self.lru_cache.cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_sc_runtime::{CondomLimits, GasCosts};
    use serial_test::serial;
    use std::sync::atomic::Ordering;
    use tempfile::TempDir;

    /// Minimal valid wasm module (an `add_one` function).
    const TEST_BYTECODE: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f, 0x01,
        0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x0b, 0x01, 0x07, 0x61, 0x64, 0x64, 0x5f, 0x6f, 0x6e,
        0x65, 0x00, 0x00, 0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x41, 0x01, 0x6a, 0x0b, 0x00,
        0x1a, 0x04, 0x6e, 0x61, 0x6d, 0x65, 0x01, 0x0a, 0x01, 0x00, 0x07, 0x61, 0x64, 0x64, 0x5f,
        0x6f, 0x6e, 0x65, 0x02, 0x07, 0x01, 0x00, 0x01, 0x00, 0x02, 0x70, 0x30,
    ];

    fn setup(max_module_length: u64) -> (ModuleCache, TempDir) {
        let cache_dir = TempDir::new().unwrap();
        let cache = ModuleCache::new(ModuleCacheConfig {
            hd_cache_path: cache_dir.path().to_path_buf(),
            gas_costs: GasCosts::default(),
            lru_cache_size: 10,
            hd_cache_size: 10,
            snip_amount: 1,
            max_module_length,
            condom_limits: CondomLimits::default(),
        });
        (cache, cache_dir)
    }

    /// When a module is already resident in the LRU cache, `save_module` must not
    /// perform a redundant RocksDB read/deserialize via the HD cache.
    #[test]
    #[serial]
    fn test_save_module_skips_hd_read_when_in_lru() {
        let (mut cache, _cache_dir) = setup(1_000_000);
        let condom_limits = CondomLimits::default();

        // First save: nothing is cached yet, so the HD cache is probed once (a miss)
        // before compiling and inserting into both caches.
        cache.save_module(TEST_BYTECODE, condom_limits.clone());
        assert_eq!(
            cache.hd_cache.read_count.load(Ordering::Relaxed),
            1,
            "first save_module should probe the HD cache exactly once"
        );

        // Second save: the module is already present in the LRU cache, so we must
        // short-circuit and never touch the HD cache for a read.
        cache.save_module(TEST_BYTECODE, condom_limits);
        assert_eq!(
            cache.hd_cache.read_count.load(Ordering::Relaxed),
            1,
            "save_module must not read the HD cache when the module is in the LRU cache"
        );
    }

    /// `load_cached_module_by_hash` must return what `load_module` returns, and
    /// `None` whenever that cannot be decided without the bytecode.
    #[test]
    #[serial]
    fn test_load_cached_module_by_hash_matches_load_module() {
        let (mut cache, _cache_dir) = setup(1_000_000);
        let condom_limits = CondomLimits::default();
        let hash = Hash::compute_from(TEST_BYTECODE);
        let len = TEST_BYTECODE.len();

        // not cached yet: the caller must fall back to the bytecode path
        assert!(cache
            .load_cached_module_by_hash(hash, len, u64::MAX, condom_limits.clone())
            .is_none());

        cache
            .load_module_with_hash(TEST_BYTECODE, hash, u64::MAX, condom_limits.clone())
            .expect("valid module");
        assert!(matches!(
            cache.load_cached_module_by_hash(hash, len, u64::MAX, condom_limits.clone()),
            Some(Ok(_))
        ));

        // not enough gas: same error as load_module
        let low_gas = cache.cfg.gas_costs.max_instance_cost - 1;
        let expected = cache
            .load_module(TEST_BYTECODE, low_gas, condom_limits.clone())
            .err()
            .expect("gas check must fail")
            .to_string();
        let got = cache
            .load_cached_module_by_hash(hash, len, low_gas, condom_limits.clone())
            .expect("gas check needs no bytecode")
            .err()
            .expect("gas check must fail")
            .to_string();
        assert_eq!(got, expected);

        // invalid lengths are left to the bytecode path, which reports them
        assert!(cache
            .load_cached_module_by_hash(hash, 0, u64::MAX, condom_limits.clone())
            .is_none());
        assert!(cache
            .load_cached_module_by_hash(hash, 1_000_001, u64::MAX, condom_limits.clone())
            .is_none());

        // a module marked invalid is reported as invalid, as by load_module
        cache.set_invalid(TEST_BYTECODE, "test error".to_string());
        let expected = cache
            .load_module(TEST_BYTECODE, u64::MAX, condom_limits.clone())
            .err()
            .expect("invalid module")
            .to_string();
        let got = cache
            .load_cached_module_by_hash(hash, len, u64::MAX, condom_limits)
            .expect("invalid module is cached")
            .err()
            .expect("invalid module")
            .to_string();
        assert_eq!(got, expected);
    }

    #[test]
    fn test_load_tmp_module_rejects_empty_bytecode() {
        let (cache, _cache_dir) = setup(4);

        let result = cache.load_tmp_module(&[], u64::MAX, CondomLimits::default());

        assert!(matches!(
            result,
            Err(CacheError::LoadError(error)) if error == "load_tmp_module: bytecode is absent"
        ));
    }

    #[test]
    fn test_load_tmp_module_rejects_oversized_bytecode() {
        let (cache, _cache_dir) = setup(4);

        // Invalid Wasm confirms the size check runs before RuntimeModule compilation.
        let result = cache.load_tmp_module(&[0; 5], u64::MAX, CondomLimits::default());

        assert!(matches!(
            result,
            Err(CacheError::LoadError(error))
                if error == "load_tmp_module: bytecode length 5 exceeds max module length 4"
        ));
    }
}
