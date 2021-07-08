use sled::transaction::TransactionError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("failed retrieving storage controller event")]
    ControllerEventError,
    #[error("join error {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("sled error {0}")]
    SledError(#[from] sled::Error),
    #[error("transaction error{0}")]
    TransactionError(#[from] TransactionError<InternalError>),
}

#[derive(Error, Debug)]
pub enum InternalError {
    #[error("transaction error {0}")]
    TransactionError(String),
}
