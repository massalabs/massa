// Copyright (c) 2021 MASSA LABS <info@massa.net>

use serde::{Deserialize, Serialize};

pub const CHANNEL_SIZE: usize = 256;

/// Pool configuration
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct PoolSettings {
    /// max pool size per thread (in number of operations)
    pub max_pool_size_per_thread: u64,
    /// how many periods in the future can an op validity start ? Otherwise op is ignored
    pub max_operation_future_validity_start_periods: u64,
    /// max endorsement we keep in pool
    pub max_endorsement_count: u64,
    pub max_item_return_count: usize,
}
