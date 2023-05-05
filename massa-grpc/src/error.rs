// Copyright (c) 2023 MASSA LABS <info@massa.net>

use std::error::Error;

use displaydoc::Display;

use massa_consensus_exports::error::ConsensusError;
use massa_execution_exports::ExecutionError;
use massa_hash::MassaHashError;
use massa_models::error::ModelsError;
use massa_protocol_exports::ProtocolError;
use massa_time::TimeError;
use massa_wallet::WalletError;
use tracing::log::error;

/// Errors of the gRPC component.
#[non_exhaustive]
#[derive(Display, thiserror::Error, Debug)]
pub enum GrpcError {
    /// `massa_hash` error: {0}
    MassaHashError(#[from] MassaHashError),
    /// consensus error: {0}
    ConsensusError(#[from] ConsensusError),
    /// execution error: {0}
    ExecutionError(#[from] ExecutionError),
    /// Protocol error: {0}
    ProtocolError(#[from] ProtocolError),
    /// Reflection error : {0}
    ReflectionError(#[from] tonic_reflection::server::Error),
    /// Models error: {0}
    ModelsError(#[from] ModelsError),
    /// Time error: {0}
    TimeError(#[from] TimeError),
    /// Wallet error: {0}
    WalletError(#[from] WalletError),
    /// Internal server error: {0}
    InternalServerError(String),
    /// Invalid argument error: {0}
    InvalidArgument(String),
}

impl From<GrpcError> for tonic::Status {
    fn from(error: GrpcError) -> Self {
        error!("{}", error);
        match error {
            GrpcError::MassaHashError(e) => tonic::Status::internal(e.to_string()),
            GrpcError::ConsensusError(e) => tonic::Status::internal(e.to_string()),
            GrpcError::ExecutionError(e) => tonic::Status::internal(e.to_string()),
            GrpcError::ProtocolError(e) => tonic::Status::internal(e.to_string()),
            GrpcError::ModelsError(e) => tonic::Status::internal(e.to_string()),
            GrpcError::TimeError(e) => tonic::Status::internal(e.to_string()),
            GrpcError::WalletError(e) => tonic::Status::internal(e.to_string()),
            GrpcError::InternalServerError(e) => tonic::Status::internal(e),
            GrpcError::ReflectionError(e) => tonic::Status::internal(e.to_string()),
            GrpcError::InvalidArgument(e) => tonic::Status::invalid_argument(e),
        }
    }
}

/// returns the first IO error found
pub fn match_for_io_error(err_status: &tonic::Status) -> Option<&std::io::Error> {
    let mut err: &(dyn Error + 'static) = err_status;

    loop {
        if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
            return Some(io_err);
        }

        // h2::Error do not expose std::io::Error with `source()`
        // https://github.com/hyperium/h2/pull/462
        if let Some(h2_err) = err.downcast_ref::<h2::Error>() {
            if let Some(io_err) = h2_err.get_io() {
                return Some(io_err);
            }
        }

        err = match err.source() {
            Some(err) => err,
            None => return None,
        };
    }
}
