// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::config::{
    DENUNCIATION_EXPIRE_CYCLE_DELTA, ENDORSEMENT_COUNT, GENESIS_TIMESTAMP, MAX_BLOCK_SIZE,
    MAX_GAS_PER_BLOCK, MAX_OPERATIONS_PER_BLOCK, OPERATION_VALIDITY_PERIODS, PERIODS_PER_CYCLE,
    ROLL_PRICE, T0, THREAD_COUNT,
};

use crate::PoolConfig;

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            thread_count: THREAD_COUNT,
            operation_validity_periods: OPERATION_VALIDITY_PERIODS,
            max_block_gas: MAX_GAS_PER_BLOCK,
            roll_price: ROLL_PRICE,
            max_block_size: MAX_BLOCK_SIZE,
            max_operation_pool_size_per_thread: 1000,
            max_endorsements_pool_size_per_thread: 1000,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            max_block_endorsement_count: ENDORSEMENT_COUNT,
            channels_size: 1024,
            broadcast_enabled: false,
            broadcast_operations_capacity: 5000,
            genesis_timestamp: *GENESIS_TIMESTAMP,
            t0: T0,
            periods_per_cycle: PERIODS_PER_CYCLE,
            denunciation_expire_cycle_delta: DENUNCIATION_EXPIRE_CYCLE_DELTA,
        }
    }
}
