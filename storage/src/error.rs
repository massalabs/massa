use models::ModelsError;
use sled::transaction::{TransactionError, UnabortableTransactionError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("sled error: {0}")]
    SledError(#[from] sled::Error),
    #[error("transaction error: {0}")]
    TransactionError(#[from] TransactionError<InternalError>),
    #[error("unabortable transaction error: {0}")]
    UnabortableTransactionError(#[from] UnabortableTransactionError),
    #[error("model error: {0}")]
    ModelError(#[from] ModelsError),
    #[error("crypto parse error: {0}")]
    CryptoParseError(#[from] crypto::CryptoError),
    #[error("Mutex poisoned error: {0}")]
    MutexPoisonedError(String),
    #[error("Database inconsistency error: {0}")]
    DatabaseInconsistency(String),
    #[error("deserialization error: {0}")]
    DeserializationError(String),
    #[error("add block error: {0}")]
    AddBlockError(String),
    #[error("operation error: {0}")]
    OperationError(String),
    #[error("clear error: {0}")]
    ClearError(String),
}

#[derive(Error, Debug)]
pub enum InternalError {
    #[error("transaction error {0}")]
    TransactionError(String),
}
