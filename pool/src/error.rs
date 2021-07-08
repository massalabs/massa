// Copyright (c) 2021 MASSA LABS <info@massa.net>

use communication::CommunicationError;
use models::ModelsError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PoolError {
    #[error("there was an inconsistency between containers")]
    ContainerInconsistency(String),
    #[error("Communication error {0}")]
    CommunicationError(#[from] CommunicationError),
    #[error("channel error : {0}")]
    ChannelError(String),
    #[error("Join error {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("models error: {0}")]
    ModelsError(#[from] ModelsError),
}
