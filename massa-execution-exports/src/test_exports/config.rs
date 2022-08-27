// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines testing tools related to the configuration

use massa_time::MassaTime;

use crate::ExecutionConfig;

/// Default value of `ExecutionConfig` used for tests
impl Default for ExecutionConfig {
    fn default() -> Self {
        use massa_models::config::default_testing::*;

        Self {
            readonly_queue_length: READONLY_QUEUE_LENGTH,
            max_final_events: MAX_FINAL_EVENTS,
            max_async_gas: MAX_ASYNC_GAS,
            thread_count: THREAD_COUNT,
            roll_price: ROLL_PRICE,
            cursor_delay: CURSOR_DELAY,
            block_reward: BLOCK_REWARD,
            endorsement_count: ENDORSEMENT_COUNT as u64,
            max_gas_per_block: MAX_GAS_PER_BLOCK,
            operation_validity_period: OPERATION_VALIDITY_PERIODS,
            periods_per_cycle: PERIODS_PER_CYCLE,
            clock_compensation: Default::default(),
            // reset genesis timestamp because we are in test mode that can take a while to process
            genesis_timestamp: MassaTime::now().expect("Impossible to reset the timestamp in test"),
            t0: 10.into(),
            stats_time_window_duration: MassaTime::from_millis(30000),
        }
    }
}
