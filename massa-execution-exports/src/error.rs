// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! this file defines all possible execution error categories

use displaydoc::Display;
use thiserror::Error;

/// Errors of the execution component.
#[non_exhaustive]
#[derive(Clone, Display, Error, Debug)]
pub enum ExecutionError {
    /// Channel error
    ChannelError(String),

    /// Runtime error: {0}
    RuntimeError(String),
}
