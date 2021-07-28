// Copyright (c) 2021 MASSA LABS <info@massa.net>

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ModelsError {
    #[error("hashing error")]
    HashError,
    #[error("Serialization error: {0}")]
    SerializeError(String),
    #[error("Deserialization error: {0}")]
    DeserializeError(String),
    #[error("buffer error: {0}")]
    BufferError(String),
    #[error("crypto error: {0}")]
    CryptoError(#[from] crypto::CryptoError),
    #[error("thread overflow error")]
    ThreadOverflowError,
    #[error("period overflow error")]
    PeriodOverflowError,
    #[error("amount parse error")]
    AmountParseError(String),
    #[error("checked operation error")]
    CheckedOperationError(String),
    #[error("invalid version identifier: {0}")]
    InavalidVersionError(String),
}
