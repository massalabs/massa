use consensus::ConsensusError;
use models::ModelsError;
use storage::StorageError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("join error:  {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("send  channel error: {0}")]
    SendChannelError(String),
    #[error("receive  channel error: {0}")]
    ReceiveChannelError(String),
    #[error("server error: {0}")]
    ServerError(#[from] warp::Error),
    #[error("consensus error : {0}")]
    ConsensusError(#[from] ConsensusError),
    #[error("not found")]
    NotFound,
    #[error("storage error: {0}")]
    StorageError(#[from] StorageError),
    #[error("data inconsistency error: {0}")]
    DataInconsistencyError(String),
    #[error("model error: {0}")]
    ModelError(#[from] ModelsError),
}
