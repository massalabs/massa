//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a configuration structure containing all settings for the asynchronous message pool system

/// Asynchronous pool configuration
#[derive(Debug, Clone)]
pub struct AsyncPoolConfig {
    /// max number of messages in the pool
    pub(crate) max_length: u64,
    /// part size (for bootstrap limits)
    pub(crate) bootstrap_part_size: u64,
    /// max async message data (for bootstrap limits)
    pub max_async_message_data: u64,
    /// thread count
    pub(crate) thread_count: u8,
}
