//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a config structure containing all settings for the async message pool system

/// Async pool configuration
#[derive(Debug, Clone)]
pub struct AsyncPoolConfig {
    /// max number of messages in the pool
    pub max_length: u64,
}
