// Copyright (c) 2021 MASSA LABS <info@massa.net>
use consensus::ConsensusError;
use crypto::CryptoError;
use models::ModelsError;
use storage::StorageError;
use thiserror::Error;
use time::TimeError;

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
    #[error("storage error : {0}")]
    StorageError(#[from] StorageError),
    #[error("crypto error : {0}")]
    CryptoError(#[from] CryptoError),
    #[error("time error : {0}")]
    TimeError(#[from] TimeError),
    #[error("models error : {0}")]
    ModelsError(#[from] ModelsError),
    #[error("not found")]
    NotFound,
    #[error("inconsistency: {0}")]
    InconsistencyError(String),
}
