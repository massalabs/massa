// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{peers::PeerType, ConnectionId};
use displaydoc::Display;
use massa_models::ModelsError;
use std::net::IpAddr;
use thiserror::Error;

/// Network error
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum NetworkError {
    /// wrong signature
    WrongSignature,
    /// Protocol err:{0}
    GeneralProtocolError(String),
    /// An error occurred during channel communication: {0}
    ChannelError(String),
    /// A tokio task has crashed err:{0}
    TokioTaskJoinError(#[from] tokio::task::JoinError),
    /// error receiving oneshot response : {0}
    TokioRecvError(#[from] tokio::sync::oneshot::error::RecvError),
    /// Error during network connection: {0}
    PeerConnectionError(#[from] NetworkConnectionErrorType),
    /// The ip:`{0}` address is not valid
    InvalidIpError(IpAddr),
    /// Active connection missing:`{0}`
    ActiveConnectionMissing(ConnectionId),
    /// IO error : {0}
    IOError(#[from] std::io::Error),
    /// Serde error : {0}
    SerdeError(#[from] serde_json::Error),
    /// `MassaHash` error {0}
    MassaHashError(#[from] massa_hash::MassaHashError),
    /// handshake error:{0:?}
    HandshakeError(HandshakeErrorType),
    /// the network controller should not drop a node command sender before shutting down the node.
    UnexpectedNodeCommandChannelClosure,
    /// the writer of a node should not drop its event sender before sending a `clean_exit` message.
    UnexpectedWriterClosure,
    /// Time error {0}
    TimeError(#[from] massa_time::TimeError),
    /// missing peers
    MissingPeersError,
    /// missing block
    MissingBlock,
    /// models error: {0}
    ModelsError(#[from] ModelsError),
    /// container inconsistency error: {0}
    ContainerInconsistencyError(String),
}

/// Handshake error type
#[derive(Debug)]
pub enum HandshakeErrorType {
    /// Id already exist
    HandshakeIdAlreadyExist(String),
    /// timeout
    HandshakeTimeout,
    /// Unexpected interruption
    HandshakeInterruption(String),
    /// wrong handshake message
    HandshakeWrongMessage,
    /// if remote id is the same as ours
    HandshakeKey,
    /// Invalid signature
    HandshakeInvalidSignature,
    /// Incompatible version
    IncompatibleVersion,
    /// Outgoing connection returned a bootstrapable peer list: {0:?}
    PeerListReceived(Vec<IpAddr>),
}

/// return handshake error
#[macro_export]
macro_rules! throw_handshake_error {
    ($err:ident) => {
        return Err(NetworkError::HandshakeError(HandshakeErrorType::$err))
    };
    ($err:ident, $e:expr) => {
        return Err(NetworkError::HandshakeError(HandshakeErrorType::$err($e)))
    };
}

pub use throw_handshake_error;

#[derive(Debug, Error, Display)]
#[non_exhaustive]
/// Incoming and outgoing connection with other peers error list
pub enum NetworkConnectionErrorType {
    /// Try to close connection with no connection: {0}
    CloseConnectionWithNoConnectionToClose(IpAddr),
    /// Peer info not found for address: {0}
    PeerInfoNotFoundError(IpAddr),
    /// Peer info not found for address: {0}
    PeerTypeNotFoundError(PeerType),
    /// Too many connection attempt: {0}
    TooManyConnectionAttempts(IpAddr),
    /// Too many connection failure: {0}
    TooManyConnectionFailure(IpAddr),
    /// Max connected peers reached: {0}
    MaxPeersConnectionReached(IpAddr),
    /// Attempt too connect from you own IP
    SelfConnection,
    /// A banned peer is trying to connect: {0}
    BannedPeerTryingToConnect(IpAddr),
    /// Unexpected error
    UnexpectedError,
}
