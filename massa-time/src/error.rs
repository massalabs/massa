// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug, Clone)]
pub enum TimeError {
    /// Error converting
    ConversionError,
    /// Time overflow error
    TimeOverflowError,
    /// Checked operation error : {0}
    CheckedOperationError(String),
}
