use consensus::ConsensusError;
use communication::CommunicationError;
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
    DataInconsistencyError(String),
    #[error("data inconsistency error: {0}")]
    ServerError(#[from] warp::Error),
    #[error("consensus module error: {0}")]
    ConsensusError(#[from] ConsensusError),
    #[error("commiunication module error: {0}")]
    CommunicationError(#[from] CommunicationError),
    #[error("not found")]
    NotFound,
}
