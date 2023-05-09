use massa_sc_runtime::GasCosts;
use std::path::PathBuf;

pub struct ModuleCacheConfig {
    /// Path to the hard drive cache storage
    pub(crate) hd_cache_path: PathBuf,
    /// Gas costs used to:
    /// * setup `massa-sc-runtime` metering on compilation
    /// * debit compilation costs
    pub(crate) gas_costs: GasCosts,
    /// Default gas for compilation
    pub(crate) compilation_gas: u64,
    /// Maximum number of entries we want to keep in the LRU cache
    pub(crate) lru_cache_size: u32,
    /// Maximum number of entries we want to keep in the HD cache
    pub(crate) hd_cache_size: usize,
    /// Amount of entries removed when `hd_cache_size` is reached
    pub(crate) snip_amount: usize,
}
