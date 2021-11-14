// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::messages::BootstrapMessage;
use consensus::error::ConsensusError;
use crypto::CryptoError;
use displaydoc::Display;
use network::NetworkError;
use thiserror::Error;
use time::TimeError;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum BootstrapError {
    /// io error: {0}
    IoError(#[from] std::io::Error),
    /// general bootstrap error: {0}
    GeneralError(String),
    /// models error: {0}
    ModelsError(#[from] models::ModelsError),
    /// unexpected message from bootstrap node: {0:?}
    UnexpectedMessage(BootstrapMessage),
    /// connection with bootstrap node dropped
    UnexpectedConnectionDrop,
    /// crypto error: {0}
    CryptoError(#[from] CryptoError),
    /// time error: {0}
    TimeError(#[from] TimeError),
    /// consensus error: {0}
    ConsensusError(#[from] ConsensusError),
    /// network error: {0}
    NetworkError(#[from] NetworkError),
    /// join error: {0}
    JoinError(#[from] tokio::task::JoinError),
    /// missing private key file
    MissingKeyError,
    /// incompatible version: {0}
    IncompatibleVersionError(String),
}
