// Copyright (c) 2021 MASSA LABS <info@massa.net>

use displaydoc::Display;
use models::ModelsError;
use sled::transaction::{TransactionError, UnabortableTransactionError};
use thiserror::Error;

#[derive(Display, Error, Debug)]
pub enum StorageError {
    /// join error: {0}
    JoinError(#[from] tokio::task::JoinError),
    /// sled error: {0}
    SledError(#[from] sled::Error),
    /// transaction error: {0}
    TransactionError(#[from] TransactionError<InternalError>),
    /// unabortable transaction error: {0}
    UnabortableTransactionError(#[from] UnabortableTransactionError),
    /// model error: {0}
    ModelError(#[from] ModelsError),
    /// crypto parse error: {0}
    CryptoParseError(#[from] crypto::CryptoError),
    /// Mutex poisoned error: {0}
    MutexPoisonedError(String),
    /// Database inconsistency error: {0}
    DatabaseInconsistency(String),
    /// deserialization error: {0}
    DeserializationError(String),
    /// add block error: {0}
    AddBlockError(String),
    /// operation error: {0}
    OperationError(String),
    /// clear error: {0}
    ClearError(String),
}

#[derive(Display, Error, Debug)]
pub enum InternalError {
    /// transaction error {0}
    TransactionError(String),
}
