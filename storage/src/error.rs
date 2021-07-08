use models::ModelsError;
use sled::transaction::TransactionError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("join error {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("sled error {0}")]
    SledError(#[from] sled::Error),
    #[error("transaction error{0}")]
    TransactionError(#[from] TransactionError<InternalError>),
    #[error("model error{0}")]
    ModelError(#[from] ModelsError),
    #[error("crypto parse error : {0}")]
    CryptoParseError(#[from] crypto::CryptoError),
    #[error("Mutex poisoned error:{0}")]
    MutexPoisonedError(String),
}

#[derive(Error, Debug)]
pub enum InternalError {
    #[error("transaction error {0}")]
    TransactionError(String),
}
