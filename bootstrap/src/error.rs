// Copyright (c) 2021 MASSA LABS <info@massa.net>

use communication::CommunicationError;
use consensus::ConsensusError;
use crypto::CryptoError;
use thiserror::Error;
use time::TimeError;

use crate::messages::BootstrapMessage;

#[derive(Error, Debug)]
pub enum BootstrapError {
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("general bootstrap error: {0}")]
    GeneralError(String),
    #[error("models error: {0}")]
    ModelsError(#[from] models::ModelsError),
    #[error("unexpected message from bootstrap node: {0:?}")]
    UnexpectedMessage(BootstrapMessage),
    #[error("connection with bootstrap node dropped")]
    UnexpectedConnectionDrop,
    #[error("crypto error: {0}")]
    CryptoError(#[from] CryptoError),
    #[error("time error: {0}")]
    TimeError(#[from] TimeError),
    #[error("consensus error: {0}")]
    ConsensusError(#[from] ConsensusError),
    #[error("communication error: {0}")]
    CommunicationError(#[from] CommunicationError),
    #[error("join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("missing private key file")]
    MissingKeyError,
    #[error("incompatible version: {0}")]
    IncompatibleVersionError(String),
}
