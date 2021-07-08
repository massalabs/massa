use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("failed retrieving storage controller event")]
    ControllerEventError,
    #[error("join error {0}")]
    JoinError(#[from] tokio::task::JoinError),
}
