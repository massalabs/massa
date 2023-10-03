//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::config::{
    MAX_ASYNC_POOL_LENGTH, MAX_DATASTORE_KEY_LENGTH, MAX_FUNCTION_NAME_LENGTH, MAX_PARAMETERS_SIZE,
    THREAD_COUNT,
};

///! This file defines testing tools related to the configuration
use crate::config::AsyncPoolConfig;

/// Default value of `AsyncPoolConfig` used for tests
impl Default for AsyncPoolConfig {
    fn default() -> Self {
        AsyncPoolConfig {
            max_length: MAX_ASYNC_POOL_LENGTH,
            max_function_length: MAX_FUNCTION_NAME_LENGTH,
            max_function_params_length: MAX_PARAMETERS_SIZE as u64,
            thread_count: THREAD_COUNT,
            max_key_length: MAX_DATASTORE_KEY_LENGTH as u32,
        }
    }
}
