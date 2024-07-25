// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use massa_serialization::SerializeError;
use thiserror::Error;
/// models error
pub type ModelsResult<T, E = ModelsError> = core::result::Result<T, E>;

/// models error
#[non_exhaustive]
#[derive(Display, Error, Debug, Clone)]
pub enum ModelsError {
    /// hashing error
    HashError,
    /// Serialization error: {0}
    SerializeError(String),
    /// Serialization error: {0}
    SerializationError(#[from] SerializeError),
    /// Deserialization error: {0}
    DeserializeError(String),
    /// buffer error: {0}
    BufferError(String),
    /// `MassaHash` error: {0}
    MassaHashError(#[from] massa_hash::MassaHashError),
    /// `massa_signature` error: {0}
    MassaSignatureError(#[from] massa_signature::MassaSignatureError),
    /// thread overflow error
    ThreadOverflowError,
    /// period overflow error
    PeriodOverflowError,
    /// amount parse error
    AmountParseError(String),
    /// address parsing error: {0}
    AddressParseError(String),
    /// node id parsing error
    NodeIdParseError,
    /// block id parsing error
    BlockIdParseError,
    /// operation id parsing error
    OperationIdParseError,
    /// endorsement id parsing error
    EndorsementIdParseError,
    /// checked operation error
    CheckedOperationError(String),
    /// invalid version identifier: {0}
    InvalidVersionError(String),
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
    /// Wrong operation id size deduced on join
    OperationPrefixJoinError,
    /// Outdated bootstrap cursor
    OutdatedBootstrapCursor,
    /// Error raised {0}
    ErrorRaised(String),
    /// deferred call id parsing error
    DeferredCallIdParseError,
}

impl From<nom::Err<nom::error::Error<&[u8]>>> for ModelsError {
    fn from(err: nom::Err<nom::error::Error<&[u8]>>) -> Self {
        ModelsError::DeserializeError(err.to_string())
    }
}

impl From<&'static str> for ModelsError {
    fn from(err: &'static str) -> Self {
        ModelsError::ErrorRaised(err.to_string())
    }
}
