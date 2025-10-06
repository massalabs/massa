// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use thiserror::Error;

/// Pool error
#[derive(Display, Error, Debug, Clone)]
pub enum PoolError {
    /// Lock timeout: failed to acquire read lock within the specified timeout period
    LockTimeout,
    /// Channel error: {0}
    ChannelError(String),
}
