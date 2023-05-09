//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the factory settings

use massa_time::MassaTime;

/// Structure defining the settings of the factory
#[derive(Debug, Clone)]
pub(crate)  struct FactoryConfig {
    /// number of threads
    pub(crate)  thread_count: u8,

    /// genesis timestamp
    pub(crate)  genesis_timestamp: MassaTime,

    /// period duration
    pub(crate)  t0: MassaTime,

    /// initial delay before starting production, to avoid double-production on node restart
    pub(crate)  initial_delay: MassaTime,

    /// maximal block size in bytes
    pub(crate)  max_block_size: u64,

    /// maximal block gas
    pub(crate)  max_block_gas: u64,

    /// maximum number of operation ids in block
    pub(crate)  max_operations_per_block: u32,

    /// last start period, to deduce genesis blocks
    pub(crate)  last_start_period: u64,

    /// cycle duration in periods
    pub(crate)  periods_per_cycle: u64,

    /// denunciation expiration as periods
    pub(crate)  denunciation_expire_periods: u64,
}
