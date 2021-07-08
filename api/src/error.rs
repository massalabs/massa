use consensus::ConsensusError;
use crypto::CryptoError;
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
    #[error("crypto error : {0}")]
    CryptoError(#[from] CryptoError),
}
