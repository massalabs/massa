// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;

use massa_consensus_exports::error::ConsensusError;
use massa_execution_exports::ExecutionError;
use massa_hash::MassaHashError;
use massa_models::error::ModelsError;
use massa_network_exports::NetworkError;
use massa_protocol_exports::ProtocolError;
use massa_time::TimeError;
use massa_wallet::WalletError;

//TODO handle custom error
/// Errors of the gRPC component.
#[non_exhaustive]
#[derive(Display, thiserror::Error, Debug)]
pub enum GrpcError {
    /// Send channel error: {0}
    SendChannelError(String),
    /// Receive channel error: {0}
    ReceiveChannelError(String),
    /// `massa_hash` error: {0}
    MassaHashError(#[from] MassaHashError),
    /// consensus error: {0}
    ConsensusError(#[from] ConsensusError),
    /// execution error: {0}
    ExecutionError(#[from] ExecutionError),
    /// Network error: {0}
    NetworkError(#[from] NetworkError),
    /// Protocol error: {0}
    ProtocolError(#[from] ProtocolError),
    /// Models error: {0}
    ModelsError(#[from] ModelsError),
    /// Time error: {0}
    TimeError(#[from] TimeError),
    /// Wallet error: {0}
    WalletError(#[from] WalletError),
    /// Not found
    NotFound,
    /// Inconsistency error: {0}
    InconsistencyError(String),
    /// Missing command sender: {0}
    MissingCommandSender(String),
    /// Missing configuration: {0}
    MissingConfig(String),
    /// The wrong API was called
    WrongAPI,
    /// Bad request: {0}
    BadRequest(String),
    /// Internal server error: {0}
    InternalServerError(String),
}
