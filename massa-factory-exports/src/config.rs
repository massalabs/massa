//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the factory settings

use massa_time::MassaTime;

/// Structure defining the settings of the factory
#[derive(Debug, Clone)]
pub struct FactoryConfig {
    /// number of threads
    pub thread_count: u8,
    /// genesis timestamp
    pub genesis_timestamp: MassaTime,
    /// period duration
    pub t0: MassaTime,
    /// initial delay before starting production, to avoid double-production on node restart
    pub initial_delay: MassaTime,
    /// maximal block size in bytes
    pub max_block_size: u64,
    /// maximal block gas
    pub max_block_gas: u64,
    /// maximum number of operation ids in block
    pub max_operations_per_block: u32,
    /// last start period, to deduce genesis blocks
    pub last_start_period: u64,
    /// cycle duration in periods
    pub periods_per_cycle: u64,
    /// denunciation expiration as periods
    pub denunciation_expire_periods: u64,
    /// choose whether to stop production when zero connections on protocol
    pub stop_production_when_zero_connections: bool,
    /// chain id
    pub chain_id: u64,
    /// warn if block production is delayed by more than this
    pub block_delay_warn: MassaTime,
    /// timeout for optional channel calls in block production
    pub block_opt_channel_timeout: MassaTime,
}
