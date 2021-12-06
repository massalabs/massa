// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::ProtocolEvent;
use displaydoc::Display;
use models::ModelsError;
use network::ConnectionId;
use network::NetworkError;
use std::net::IpAddr;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum ProtocolError {
    /// wrong signature
    WrongSignature,
    /// Protocol err:{0}
    GeneralProtocolError(String),
    /// An error occurred during channel communication: {0}
    ChannelError(String),
    /// A tokio task has crashed err:{0}
    TokioTaskJoinError(#[from] tokio::task::JoinError),
    /// error receiving oneshot response : {0}
    TokieRecvError(#[from] tokio::sync::oneshot::error::RecvError),
    /// error sending protocol event: {0}
    TokioSendError(#[from] tokio::sync::mpsc::error::SendError<ProtocolEvent>),
    /// Error during network connection:`{0:?}`
    PeerConnectionError(NetworkConnectionErrorType),
    /// The ip:`{0}` address is not valid
    InvalidIpError(IpAddr),
    /// Active connection missing:`{0}`
    ActiveConnectionMissing(ConnectionId),
    /// IO error : {0}
    IOError(#[from] std::io::Error),
    /// Serde error : {0}
    SerdeError(#[from] serde_json::Error),
    /// massa_hash error {0}
    MassaHashError(#[from] massa_hash::MassaHashError),
    /// handshake error:{0:?}
    HandshakeError(HandshakeErrorType),
    /// the network controller should not drop a node command sender before shutting down the node.
    UnexpectedNodeCommandChannelClosure,
    /// the writer of a node should not drop its event sender before sending a clean_exit message.
    UnexpectedWriterClosure,
    /// Time error {0}
    TimeError(#[from] time::TimeError),
    /// missing peers
    MissingPeersError,
    /// models error: {0}
    ModelsError(#[from] ModelsError),
    /// network error: {0}
    NetworkError(#[from] NetworkError),
    /// container inconsistency error: {0}
    ContainerInconsistencyError(String),
}

#[derive(Debug)]
pub enum HandshakeErrorType {
    HandshakeIdAlreadyExistError(String),
    HandshakeTimeoutError,
    HandshakeInterruptionError(String),
    HandshakeWrongMessageError,
    HandshakeKeyError,
    HandshakeInvalidSignatureError,
    IncompatibleVersionError,
}

#[derive(Debug)]
pub enum NetworkConnectionErrorType {
    CloseConnectionWithNoConnectionToClose(IpAddr),
    PeerInfoNotFoundError(IpAddr),
    ToManyConnectionAttempt(IpAddr),
    ToManyConnectionFailure(IpAddr),
}
