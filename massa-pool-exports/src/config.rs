//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::num::NonZeroU8;

use massa_models::amount::Amount;
use serde::{Deserialize, Serialize};

/// Pool configuration
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct PoolConfig {
    /// thread count
    pub thread_count: NonZeroU8,
    /// maximal total block operations size
    pub max_block_size: u32,
    /// maximal gas per block
    pub max_block_gas: u64,
    /// cost (in coins) of a single roll
    pub roll_price: Amount,
    /// operation validity periods
    pub operation_validity_periods: u64,
    /// max operations per block
    pub max_operations_per_block: u32,
    /// max operation pool size per thread (in number of operations)
    pub max_operation_pool_size_per_thread: usize,
    /// max endorsement pool size per thread (in number of endorsements)
    pub max_endorsements_pool_size_per_thread: usize,
    /// max number of endorsements per block
    pub max_block_endorsement_count: u32,
    /// operations and endorsements communication channels size
    pub channels_size: usize,
    /// Whether WebSockets are enabled
    pub broadcast_enabled: bool,
    /// operations sender(channel) capacity
    pub broadcast_operations_capacity: usize,
}
