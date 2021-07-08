use serde::Deserialize;

pub const CHANNEL_SIZE: usize = 256;

/// Pool configuration
#[derive(Debug, Deserialize, Clone)]
pub struct PoolConfig {
    pub operation_validity_periods: u64,
    pub max_pool_size_per_thread: u64,
}
