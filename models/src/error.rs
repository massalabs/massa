// Copyright (c) 2021 MASSA LABS <info@massa.net>

use displaydoc::Display;
use thiserror::Error;

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
    /// crypto error: {0}
    CryptoError(#[from] crypto::CryptoError),
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
}
