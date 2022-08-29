//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a configuration structure containing all settings for the asynchronous message pool system

/// Asynchronous pool configuration
#[derive(Debug, Clone)]
pub struct AsyncPoolConfig {
    /// max number of messages in the pool
    pub max_length: u64,
    /// part size message bytes (for bootstrap limits)
    pub part_size_message_bytes: u64,
    /// max data async message (for bootstrap limits)
    pub max_data_async_message: u64,
    /// thread count
    pub thread_count: u8,
}
