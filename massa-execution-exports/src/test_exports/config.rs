// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines testing tools related to the configuration

use crate::ExecutionConfig;
use massa_models::config::*;
use massa_time::MassaTime;

impl Default for ExecutionConfig {
    /// default config used for testing
    fn default() -> Self {
        Self {
            readonly_queue_length: 100,
            max_final_events: 1000,
            max_async_gas: MAX_ASYNC_GAS,
            thread_count: THREAD_COUNT,
            roll_price: ROLL_PRICE,
            cursor_delay: MassaTime::from_millis(0),
            block_reward: BLOCK_REWARD,
            endorsement_count: ENDORSEMENT_COUNT as u64,
            max_gas_per_block: MAX_GAS_PER_BLOCK,
            operation_validity_period: OPERATION_VALIDITY_PERIODS,
            periods_per_cycle: PERIODS_PER_CYCLE,
            clock_compensation: Default::default(),
            // reset genesis timestamp because we are in test mode that can take a while to process
            genesis_timestamp: MassaTime::now(0)
                .expect("Impossible to reset the timestamp in test"),
            t0: 10.into(),
            stats_time_window_duration: MassaTime::from_millis(30000),
            max_miss_ratio: *POS_MISS_RATE_DEACTIVATION_THRESHOLD,
        }
    }
}
