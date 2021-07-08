use crate::{network::ConnectionId, protocol::ProtocolEvent};
use models::ModelsError;
use std::net::IpAddr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommunicationError {
    #[error("wrong signature")]
    WrongSignature,
    #[error("Protocol err:{0}")]
    GeneralProtocolError(String),
    #[error("An error occured during channel communication: {0}")]
    ChannelError(String),
    #[error("A tokio task has crashed err:{0}")]
    TokioTaskJoinError(#[from] tokio::task::JoinError),
    #[error("error receiving oneshot response : {0}")]
    TokieRecvError(#[from] tokio::sync::oneshot::error::RecvError),
    #[error("error sending protocol event: {0}")]
    TokioSendError(#[from] tokio::sync::mpsc::error::SendError<ProtocolEvent>),
    #[error("Error during network connection:`{0:?}`")]
    PeerConnectionError(NetworkConnectionErrorType),
    #[error("The ip:`{0}` address is not valid")]
    InvalidIpError(IpAddr),
    #[error("Active connection missing:`{0}`")]
    ActiveConnectionMissing(ConnectionId),
    #[error("Event from missign node")]
    MissingNodeError,
    #[error("IO error : {0}")]
    IOError(#[from] std::io::Error),
    #[error("Serde error : {0}")]
    SerdeError(#[from] serde_json::Error),
    #[error("crypto error {0}")]
    CryptoError(#[from] crypto::CryptoError),
    #[error("handhsake error:{0:?}")]
    HandshakeError(HandshakeErrorType),
    #[error("the protocol controller should have close everything before shuting down")]
    UnexpectedProtocolControllerClosureError,
    #[error("Time error {0}")]
    TimeError(#[from] time::TimeError),
    #[error("missing peers")]
    MissingPeersError,
    #[error("Could not hash block header: {0}")]
    HeaderHashError(#[from] ModelsError),
}

#[derive(Debug)]
pub enum HandshakeErrorType {
    HandshakeIdAlreadyExistError(String),
    HandshakeTimeoutError,
    HandshakeInterruptionError,
    HandshakeWrongMessageError,
    HandshakeKeyError,
    HandshakeInvalidSignatureError,
}

#[derive(Debug)]
pub enum NetworkConnectionErrorType {
    CloseConnectionWithNoConnectionToClose(IpAddr),
    PeerInfoNotFoundError(IpAddr),
    ToManyConnectionAttempt(IpAddr),
    ToManyConnectionFailure(IpAddr),
}
