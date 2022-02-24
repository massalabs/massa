// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use massa_models::ModelsError;
use massa_protocol_exports::ProtocolError;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum PoolError {
    /// there was an inconsistency between containers
    ContainerInconsistency(String),
    /// Protocol error {0}
    ProtocolError(#[from] Box<ProtocolError>),
    /// channel error : {0}
    ChannelError(String),
    /// Join error {0}
    JoinError(#[from] tokio::task::JoinError),
    /// models error: {0}
    ModelsError(#[from] ModelsError),
}

impl From<ProtocolError> for PoolError {
    fn from(protocol_error: ProtocolError) -> Self {
        PoolError::ProtocolError(Box::new(protocol_error))
    }
}
