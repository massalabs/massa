// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use massa_models::error::ModelsError;
use massa_pos_exports::PosError;
use massa_versioning::versioning_factory::FactoryError;
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
    /// Error during network connection: `{0:?}`
    PeerConnectionError(NetworkConnectionErrorType),
    /// The ip: `{0}` address is not valid
    InvalidIpError(IpAddr),
    /// IO error: {0}
    IOError(#[from] std::io::Error),
    /// Serde error: {0}
    SerdeError(#[from] serde_json::Error),
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
    /// Send error: {0}
    SendError(String),
    /// Container inconsistency error: {0}
    ContainerInconsistencyError(String),
    /// Invalid operation error: {0}
    InvalidOperationError(String),
    /// Listener error: {0}
    ListenerError(String),
    /// Incompatible newtork version: local current is {local} received is {received}
    IncompatibleNetworkVersion {
        /// local current version
        local: u32,
        /// received version from incoming header
        received: u32,
    },
    /// Versioned factory error: {0}
    FactoryError(#[from] FactoryError),
    /// PoS error: {0}
    PosError(#[from] PosError),
}

#[derive(Debug)]
pub enum NetworkConnectionErrorType {
    CloseConnectionWithNoConnectionToClose(IpAddr),
    PeerInfoNotFoundError(IpAddr),
    ToManyConnectionAttempt(IpAddr),
    ToManyConnectionFailure(IpAddr),
}
