//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::config::{
    ASYNC_POOL_BOOTSTRAP_PART_SIZE, MAX_ASYNC_POOL_LENGTH, MAX_DATA_ASYNC_MESSAGE, THREAD_COUNT,
};

///! This file defines testing tools related to the configuration
use crate::config::AsyncPoolConfig;

/// Default value of `AsyncPoolConfig` used for tests
impl Default for AsyncPoolConfig {
    fn default() -> Self {
        AsyncPoolConfig {
            max_length: MAX_ASYNC_POOL_LENGTH,
            max_data_async_message: MAX_DATA_ASYNC_MESSAGE,
            bootstrap_part_size: ASYNC_POOL_BOOTSTRAP_PART_SIZE,
            thread_count: THREAD_COUNT,
        }
    }
}
