// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use thiserror::Error;

pub type ModelsResult<T, E = ModelsError> = core::result::Result<T, E>;

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
    TimeError(#[from] massa_time::TimeError),
    /// invalid roll update: {0}
    InvalidRollUpdate(String),
    /// Ledger changes, Amount overflow
    AmountOverflowError,
    /// Wrong prefix for hash: expected {0}, got {1}
    WrongPrefix(String, String),
}
