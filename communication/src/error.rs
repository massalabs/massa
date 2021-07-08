use crate::network::ConnectionId;
use models::error::ModelsError;
use std::net::IpAddr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommunicationError {
    #[error("Protocol err:{0}")]
    GeneralProtocolError(String),
    #[error("An error occured during channel communication: {0}")]
    ChannelError(String),
    #[error("A tokio task has crashed err:{0}")]
    TokioTaskJoinError(#[from] tokio::task::JoinError),
    #[error("Error during netwrok connection:`{0:?}`")]
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
    #[error("Flexbuffer error {0}")]
    FlexbufferError(#[from] FlexbufferError),
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

#[derive(Error, Debug)]
pub enum FlexbufferError {
    #[error("Flexbuffer serialisation error {0}")]
    FlexbufferSerializationError(#[from] flexbuffers::SerializationError),
    #[error("Flexbuffer reader error {0}")]
    FlexbufferReaderError(#[from] flexbuffers::ReaderError),
    #[error("Flexbuffer deserialisation error {0}")]
    FlexbufferDeserializationError(#[from] flexbuffers::DeserializationError),
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
