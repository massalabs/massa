// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use massa_models::error::ModelsError;
use thiserror::Error;

/// versioning error
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum VersioningMiddlewareError {
    /// An error occurred during channel communication: {0}
    ChannelError(String),
    /// Models error: {0}
    ModelsError(#[from] ModelsError),
}
