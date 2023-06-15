// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::config::{
    DENUNCIATION_EXPIRE_PERIODS, ENDORSEMENT_COUNT, MAX_BLOCK_SIZE,
    MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MAX_GAS_PER_BLOCK, MAX_OPERATIONS_PER_BLOCK,
    OPERATION_VALIDITY_PERIODS, PERIODS_PER_CYCLE, ROLL_PRICE, T0, THREAD_COUNT,
};
use massa_time::MassaTime;

use crate::PoolConfig;

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            thread_count: THREAD_COUNT,
            operation_validity_periods: OPERATION_VALIDITY_PERIODS,
            max_block_gas: MAX_GAS_PER_BLOCK,
            roll_price: ROLL_PRICE,
            max_block_size: MAX_BLOCK_SIZE,
            max_operation_pool_size: 32000,
            max_endorsements_pool_size_per_thread: 1000,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            max_block_endorsement_count: ENDORSEMENT_COUNT,
            operations_channel_size: 1024,
            endorsements_channel_size: 1024,
            denunciations_channel_size: 1024,
            broadcast_enabled: false,
            broadcast_endorsements_channel_capacity: 2000,
            broadcast_operations_channel_capacity: 5000,
            genesis_timestamp: MassaTime::now().unwrap(),
            t0: T0,
            periods_per_cycle: PERIODS_PER_CYCLE,
            denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: 0,
            operation_pool_refresh_interval: MassaTime::from_millis(2000),
            operation_max_future_start_delay: T0.saturating_mul(5),
        }
    }
}
