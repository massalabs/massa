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
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum ApiError {
    /// Send channel error: {0}
    SendChannelError(String),
    /// Receive channel error: {0}
    ReceiveChannelError(String),
    /// `massa_hash` error: {0}
    MassaHashError(#[from] MassaHashError),
    /// Consensus error: {0}
    ConsensusError(#[from] Box<ConsensusError>),
    /// Execution error: {0}
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
    /// The wrong API (either Public or Private) was called
    WrongAPI,
    /// Bad request: {0}
    BadRequest(String),
}

impl From<ApiError> for jsonrpc_core::Error {
    fn from(err: ApiError) -> Self {
        // JSON-RPC Server errors codes must be between -32099 to -32000
        let code = match err {
            ApiError::BadRequest(_) => -32000,
            ApiError::NotFound => -32004,
            ApiError::SendChannelError(_) => -32006,
            ApiError::ReceiveChannelError(_) => -32007,
            ApiError::MassaHashError(_) => -32008,
            ApiError::ConsensusError(_) => -32009,
            ApiError::ExecutionError(_) => -32010,
            ApiError::NetworkError(_) => -32011,
            ApiError::ProtocolError(_) => -32012,
            ApiError::ModelsError(_) => -32013,
            ApiError::TimeError(_) => -32014,
            ApiError::WalletError(_) => -32015,
            ApiError::InconsistencyError(_) => -32016,
            ApiError::MissingCommandSender(_) => -32017,
            ApiError::MissingConfig(_) => -32018,
            ApiError::WrongAPI => -32019,
        };
        jsonrpc_core::Error {
            code: jsonrpc_core::ErrorCode::ServerError(code),
            message: err.to_string(),
            data: None,
        }
    }
}

impl std::convert::From<ConsensusError> for ApiError {
    fn from(err: ConsensusError) -> Self {
        ApiError::ConsensusError(Box::new(err))
    }
}
