//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::config::{
    ASYNC_POOL_BOOTSTRAP_PART_SIZE, MAX_ASYNC_MESSAGE_DATA, MAX_ASYNC_POOL_LENGTH,
    MAX_DATASTORE_KEY_LENGTH, THREAD_COUNT,
};

///! This file defines testing tools related to the configuration
use crate::config::AsyncPoolConfig;

/// Default value of `AsyncPoolConfig` used for tests
impl Default for AsyncPoolConfig {
    fn default() -> Self {
        AsyncPoolConfig {
            max_length: MAX_ASYNC_POOL_LENGTH,
            max_async_message_data: MAX_ASYNC_MESSAGE_DATA,
            bootstrap_part_size: ASYNC_POOL_BOOTSTRAP_PART_SIZE,
            thread_count: THREAD_COUNT,
            max_key_length: MAX_DATASTORE_KEY_LENGTH as u32,
        }
    }
}
