// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::ProtocolEvent;
use displaydoc::Display;
use massa_models::ModelsError;
use massa_network_exports::ConnectionId;
use massa_network_exports::NetworkError;
use std::net::IpAddr;
use thiserror::Error;

/// protocol error
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
    /// error receiving one shot response : {0}
    TokioRecvError(#[from] tokio::sync::oneshot::error::RecvError),
    /// error sending protocol event: {0}
    TokioSendError(#[from] Box<tokio::sync::mpsc::error::SendError<ProtocolEvent>>),
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
    /// `massa_hash` error {0}
    MassaHashError(#[from] massa_hash::MassaHashError),
    /// the network controller should not drop a node command sender before shutting down the node.
    UnexpectedNodeCommandChannelClosure,
    /// the writer of a node should not drop its event sender before sending a `clean_exit` message.
    UnexpectedWriterClosure,
    /// Time error {0}
    TimeError(#[from] massa_time::TimeError),
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
pub enum NetworkConnectionErrorType {
    CloseConnectionWithNoConnectionToClose(IpAddr),
    PeerInfoNotFoundError(IpAddr),
    ToManyConnectionAttempt(IpAddr),
    ToManyConnectionFailure(IpAddr),
}
