//! Default values for testing configuration of pool module
use crate::{settings::PoolConfig, PoolSettings};
lazy_static::lazy_static! {
    pub static ref POOL_CONFIG: PoolConfig = Default::default();
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            thread_count: 2,
            operation_validity_periods: 50,
            settings: PoolSettings::default(),
        }
    }
}

impl Default for PoolSettings {
    fn default() -> Self {
        Self {
            max_pool_size_per_thread: 10,
            max_operation_future_validity_start_periods: 200,
            max_endorsement_count: 1000,
            max_item_return_count: 1000,
        }
    }
}
