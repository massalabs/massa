// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use massa_models::error::ModelsError;
use massa_network_exports::ConnectionId;
use massa_network_exports::NetworkError;
use std::net::IpAddr;
use thiserror::Error;

/// protocol error
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum ProtocolError {
    /// Wrong signature
    WrongSignature,
    /// Protocol error: {0}
    GeneralProtocolError(String),
    /// An error occurred during channel communication: {0}
    ChannelError(String),
    /// A tokio task has crashed err: {0}
    TokioTaskJoinError(#[from] tokio::task::JoinError),
    /// Error receiving one shot response: {0}
    TokioRecvError(#[from] tokio::sync::oneshot::error::RecvError),
    /// Error during network connection: `{0:?}`
    PeerConnectionError(NetworkConnectionErrorType),
    /// The ip: `{0}` address is not valid
    InvalidIpError(IpAddr),
    /// Active connection missing: `{0}`
    ActiveConnectionMissing(ConnectionId),
    /// IO error: {0}
    IOError(#[from] std::io::Error),
    /// Serde error: {0}
    SerdeError(#[from] serde_json::Error),
    /// `massa_hash` error: {0}
    MassaHashError(#[from] massa_hash::MassaHashError),
    /// The network controller should not drop a node command sender before shutting down the node.
    UnexpectedNodeCommandChannelClosure,
    /// The writer of a node should not drop its event sender before sending a `clean_exit` message.
    UnexpectedWriterClosure,
    /// Time error: {0}
    TimeError(#[from] massa_time::TimeError),
    /// Missing peers
    MissingPeersError,
    /// Models error: {0}
    ModelsError(#[from] ModelsError),
    /// Network error: {0}
    NetworkError(#[from] NetworkError),
    /// Container inconsistency error: {0}
    ContainerInconsistencyError(String),
    /// Invalid operation error: {0}
    InvalidOperationError(String),
    /// Incompatible newtork version: local current is {current} received is {received}
    IncompatibleNetworkVersion {
        /// local current version
        current: u32,
        /// received version from incoming header
        received: u32,
    },
}

#[derive(Debug)]
pub enum NetworkConnectionErrorType {
    CloseConnectionWithNoConnectionToClose(IpAddr),
    PeerInfoNotFoundError(IpAddr),
    ToManyConnectionAttempt(IpAddr),
    ToManyConnectionFailure(IpAddr),
}
