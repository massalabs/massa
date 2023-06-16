//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::amount::Amount;
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};

/// Pool configuration
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct PoolConfig {
    /// thread count
    pub thread_count: u8,
    /// maximal total block operations size
    pub max_block_size: u32,
    /// maximal gas per block
    pub max_block_gas: u64,
    /// cost (in coins) of a single roll
    pub roll_price: Amount,
    /// operation validity periods
    pub operation_validity_periods: u64,
    /// operation pool refresh interval
    pub operation_pool_refresh_interval: MassaTime,
    /// max delay in the future for operation validity start
    pub operation_max_future_start_delay: MassaTime,
    /// max operations per block
    pub max_operations_per_block: u32,
    /// max operation pool size per thread (in number of operations)
    pub max_operation_pool_size: usize,
    /// max endorsement pool size per thread (in number of endorsements)
    pub max_endorsements_pool_size_per_thread: usize,
    /// max number of endorsements per block
    pub max_block_endorsement_count: u32,
    /// operations channel capacity
    pub operations_channel_size: usize,
    /// endorsements channel capacity
    pub endorsements_channel_size: usize,
    /// denunciations channel capacity
    pub denunciations_channel_size: usize,
    /// whether operations broadcast is enabled
    pub broadcast_enabled: bool,
    /// endorsements channel capacity
    pub broadcast_endorsements_channel_capacity: usize,
    /// operations channel capacity
    pub broadcast_operations_channel_capacity: usize,
    /// genesis timestamp
    pub genesis_timestamp: MassaTime,
    /// period duration
    pub t0: MassaTime,
    /// cycle duration in periods
    pub periods_per_cycle: u64,
    /// denunciation expiration (in periods)
    pub denunciation_expire_periods: u64,
    /// max number of denunciations that can be included in a block header
    pub max_denunciations_per_block_header: u32,
    /// last_start_period
    /// * If start all new network: set to 0
    /// * If from snapshot: retrieve from args
    /// * If from bootstrap: set during bootstrap
    pub last_start_period: u64,
}
