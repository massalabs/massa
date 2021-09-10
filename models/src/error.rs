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
    #[error("invalid ledger change: {0}")]
    InvalidLedgerChange(String),
}

#[derive(Error, Debug)]
pub enum ReplError {
    #[error("Error: {0}")]
    GeneralError(String),
    #[error("Error during command parsing")]
    ParseCommandError,
    #[error("Error command: {0} not found")]
    CommandNotFoundError(String),
    #[error("Node connection error err: {0}")]
    NodeConnectionError(#[from] reqwest::Error),
    #[error("Bad input parameter: {0}")]
    BadCommandParameter(String),
    #[error("Error can't create address from specified hash: {0}")]
    AddressCreationError(String),
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    JSONError(#[from] serde_json::Error),
    #[error("Serde Sq error: {0}")]
    SerdeqsError(#[from] serde_qs::Error),
    #[error("Str Format error: {0}")]
    FmtError(#[from] std::fmt::Error),
}
