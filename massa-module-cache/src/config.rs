use massa_sc_runtime::GasCosts;
use std::path::PathBuf;

pub struct ModuleCacheConfig {
    /// Path to the hard drive cache storage
    pub hd_cache_path: PathBuf,
    /// Gas costs used to:
    /// * setup `massa-sc-runtime` metering on compilation
    /// * IMPORTANT TODO: debit compilation costs
    pub gas_costs: GasCosts,
    /// Default gas for compilation
    pub compilation_gas: u64,
    /// Maximum number of entries we want to keep in the LRU cache
    pub lru_cache_size: u32,
    /// Maximum number of entries we want to keep in the HD cache
    pub hd_cache_size: usize,
    /// Amount of entries removed when `hd_cache_size` is reached
    pub snip_amount: usize,
}
