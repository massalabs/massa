// Copyright (c) 2022 MASSA LABS <info@massa.net>
use displaydoc::Display;
use massa_execution_exports::ExecutionError;
use massa_graph::error::GraphError;
use massa_models::error::ModelsError;
use massa_protocol_exports::ProtocolError;
use thiserror::Error;

use crate::events::ConsensusEvent;

/// Consensus
pub type ConsensusResult<T, E = ConsensusError> = core::result::Result<T, E>;

/// Internal error
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum InternalError {
    /// transaction error {0}
    TransactionError(String),
}

/// Consensus errors
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum ConsensusError {
    /// execution error: {0}
    ExecutionError(#[from] ExecutionError),
    /// models error: {0}
    ModelsError(#[from] ModelsError),
    /// configuration error: {0}
    ConfigError(String),
    /// Protocol error {0}
    ProtocolError(#[from] Box<ProtocolError>),
    /// failed retrieving consensus controller event
    ControllerEventError,
    /// Join error {0}
    JoinError(#[from] tokio::task::JoinError),
    /// Time error {0}
    TimeError(#[from] massa_time::TimeError),
    /// there was an inconsistency between containers {0}
    ContainerInconsistency(String),
    /// Send  channel error : {0}
    SendChannelError(String),
    /// Receive  channel error : {0}
    ReceiveChannelError(String),
    /// io error {0}
    IOError(#[from] std::io::Error),
    /// missing block {0}
    MissingBlock(String),
    /// block creation error {0}
    BlockCreationError(String),
    /// error sending consensus event: {0}
    TokioSendError(#[from] tokio::sync::mpsc::error::SendError<ConsensusEvent>),
    /// channel error: {0}
    ChannelError(String),
    /// Graph error: {0}
    GraphError(#[from] GraphError),
    /// slot overflow
    SlotOverflowError,
    /// `MassaCipher` error: {0}
    MassaCipherError(#[from] massa_cipher::CipherError),
}

impl std::convert::From<massa_protocol_exports::ProtocolError> for ConsensusError {
    fn from(err: massa_protocol_exports::ProtocolError) -> Self {
        ConsensusError::ProtocolError(Box::new(err))
    }
}
