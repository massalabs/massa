// Copyright (c) 2021 MASSA LABS <info@massa.net>

use displaydoc::Display;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum ModelsError {
    /// hashing error
    HashError,
    /// Serialization error: {0}
    SerializeError(String),
    /// Deserialization error: {0}
    DeserializeError(String),
    /// buffer error: {0}
    BufferError(String),
    /// massa_hash error: {0}
    MassaHashError(#[from] massa_hash::MassaHashError),
    /// thread overflow error
    ThreadOverflowError,
    /// period overflow error
    PeriodOverflowError,
    /// amount parse error
    AmountParseError(String),
    /// checked operation error
    CheckedOperationError(String),
    /// invalid version identifier: {0}
    InavalidVersionError(String),
    /// invalid ledger change: {0}
    InvalidLedgerChange(String),
    /// Time overflow error
    TimeOverflowError,
    /// Time error {0}
    TimeError(#[from] time::TimeError),
}
