//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines all error types for final state management

use displaydoc::Display;
use thiserror::Error;

/// Massa DB error
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum MassaDBError {
    /// invalid ChangeID: {0}
    InvalidChangeID(String),
    /// time error: {0}
    TimeError(String),
    /// rocks db error: {0}
    RocksDBError(String),
}
