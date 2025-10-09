// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::FactoryConfig;
use massa_time::MassaTime;

impl Default for FactoryConfig {
    fn default() -> Self {
        use massa_models::config::*;
        FactoryConfig {
            thread_count: THREAD_COUNT,
            genesis_timestamp: MassaTime::now(),
            t0: T0,
            initial_delay: MassaTime::from_millis(0),
            max_block_size: MAX_BLOCK_SIZE as u64,
            max_block_gas: MAX_GAS_PER_BLOCK,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            last_start_period: 0,
            periods_per_cycle: PERIODS_PER_CYCLE,
            denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
            stop_production_when_zero_connections: false,
            chain_id: *CHAINID,
            block_delay_warn: MassaTime::from_millis(500),
            block_opt_channel_timeout: MassaTime::from_millis(1000),
        }
    }
}
