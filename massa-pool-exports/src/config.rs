//! Copyright (c) 2022 MASSA LABS <info@massa.net>

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
    /// operation validity periods
    pub operation_validity_periods: u64,
    /// max operation pool size per thread (in number of operations)
    pub max_operation_pool_size_per_thread: usize,
    /// max endorsement pool size per thread (in number of endorsements)
    pub max_endorements_pool_size_per_thread: usize,
    /// max number of endorsements per block
    pub max_block_endorsement_count: u32,
}
