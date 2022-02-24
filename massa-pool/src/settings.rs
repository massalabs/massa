// Copyright (c) 2022 MASSA LABS <info@massa.net>

use serde::{Deserialize, Serialize};

/// Pool configuration
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct PoolConfig {
    /// Configuration set in file configuration (ex: `config.toml`)
    pub settings: PoolSettings,
    /// thread count
    pub thread_count: u8,
    /// operation validity periods
    pub operation_validity_periods: u64,
}

/// Pool configuration, read from a file configuration
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct PoolSettings {
    /// max pool size per thread (in number of operations)
    pub max_pool_size_per_thread: u64,
    /// how many periods in the future can an op validity start ? Otherwise op is ignored
    pub max_operation_future_validity_start_periods: u64,
    /// max endorsement we keep in pool
    pub max_endorsement_count: u64,
    /// Maximum number of item the pool can pop at a time
    pub max_item_return_count: usize,
}
