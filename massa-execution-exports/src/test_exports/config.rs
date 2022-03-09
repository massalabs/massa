// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines testing tools related to the config

use massa_time::MassaTime;

use crate::ExecutionConfig;

/// Default value of ExecutionConfig used for tests
impl Default for ExecutionConfig {
    fn default() -> Self {
        ExecutionConfig {
            readonly_queue_length: 10,
            max_final_events: 10,
            thread_count: 2,
            cursor_delay: 0.into(),
            clock_compensation: 0,
            genesis_timestamp: MassaTime::now().unwrap(),
            t0: 1000.into(),
        }
    }
}
