// Copyright (c) 2021 MASSA LABS <info@massa.net>

use communication::CommunicationError;
use displaydoc::Display;
use models::ModelsError;
use thiserror::Error;

#[derive(Display, Error, Debug)]
pub enum PoolError {
    /// there was an inconsistency between containers
    ContainerInconsistency(String),
    /// Communication error {0}
    CommunicationError(#[from] CommunicationError),
    /// channel error : {0}
    ChannelError(String),
    /// Join error {0}
    JoinError(#[from] tokio::task::JoinError),
    /// models error: {0}
    ModelsError(#[from] ModelsError),
}
