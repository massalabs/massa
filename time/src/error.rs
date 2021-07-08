// Copyright (c) 2021 MASSA LABS <info@massa.net>

use thiserror::Error;

#[derive(Error, Debug)]
pub enum TimeError {
    #[error("Error converting")]
    ConversionError,
    #[error("Time overflow error")]
    TimeOverflowError,
    #[error("Checked operation error : {0}")]
    CheckedOperationError(String),
}
