//! Copyright (c) 2022 MASSA LABS <info@massa.net>

///! This file defines testing tools related to the configuration
use crate::config::AsyncPoolConfig;

/// Default value of `AsyncPoolConfig` used for tests
impl Default for AsyncPoolConfig {
    fn default() -> Self {
        AsyncPoolConfig { max_length: 100 }
    }
}
