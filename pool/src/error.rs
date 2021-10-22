// Copyright (c) 2021 MASSA LABS <info@massa.net>

use displaydoc::Display;
use models::ModelsError;
use protocol::ProtocolError;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum PoolError {
    /// there was an inconsistency between containers
    ContainerInconsistency(String),
    /// Protocol error {0}
    ProtocolError(#[from] ProtocolError),
    /// channel error : {0}
    ChannelError(String),
    /// Join error {0}
    JoinError(#[from] tokio::task::JoinError),
    /// models error: {0}
    ModelsError(#[from] ModelsError),
}
