// Copyright (c) 2021 MASSA LABS <info@massa.net>

use consensus::error::ConsensusError;
use crypto::CryptoError;
use displaydoc::Display;
use models::ModelsError;
use storage::StorageError;
use thiserror::Error;
use time::TimeError;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum ApiError {
    /// join error:  {0}
    JoinError(#[from] tokio::task::JoinError),
    /// send  channel error: {0}
    SendChannelError(String),
    /// receive  channel error: {0}
    ReceiveChannelError(String),
    /// server error: {0}
    ServerError(#[from] warp::Error),
    /// consensus error : {0}
    ConsensusError(#[from] ConsensusError),
    /// storage error : {0}
    StorageError(#[from] StorageError),
    /// crypto error : {0}
    CryptoError(#[from] CryptoError),
    /// time error : {0}
    TimeError(#[from] TimeError),
    /// models error : {0}
    ModelsError(#[from] ModelsError),
    /// not found
    NotFound,
    /// inconsistency: {0}
    InconsistencyError(String),
}
