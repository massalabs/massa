use crate::network::default_network_controller::NetworkCommand;
use crate::network::network_controller::ConnectionId;
use crate::network::PeerInfo;
use crate::protocol::default_protocol_controller::NodeCommand;
use crate::protocol::default_protocol_controller::NodeEvent;
use crate::protocol::default_protocol_controller::ProtocolCommand;
use crate::protocol::default_protocol_controller::ProtocolEvent;
use crate::protocol::messages::Message;
use std::collections::HashMap;
use std::net::IpAddr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommunicationError {
    #[error("Protocol err:{0}")]
    GeneralProtocolError(String),
    #[error("An error occurs during channel communication err:{0}")]
    InternalChannelError(#[from] ChannelError),
    #[error("A tokio task has crashed err:{0}")]
    TokioTaskJoinError(#[from] tokio::task::JoinError),
    #[error("Error during netwrok connection:`{0:?}`")]
    PeerConnectionError(NetworkConnectionErrorType),
    #[error("The ip:`{0}` address is not valid")]
    InvalidIpError(IpAddr),
    #[error("Active connection missing:`{0}`")]
    ActiveConnectionMissing(ConnectionId),
    #[error("Mock error {0}")]
    MockError(String),
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
}

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("The network event channel return an error:{0}")]
    NetworkEventChannelError(String),
    #[error("PeerInfo channel has disconnected err:{0}")]
    PeerInfoChannelError(#[from] tokio::sync::mpsc::error::SendError<PeerInfo>),
    #[error("GetAdvertisablePeerList event oneshot channel err:{0}")]
    GetAdvertisablePeerListChannelError(String),
    #[error("SaverWatcher channel has disconnected err:{0}")]
    SaverWatcherChannelError(
        #[from] tokio::sync::watch::error::SendError<HashMap<IpAddr, PeerInfo>>,
    ),
    #[error("Protocol event channel err:{0}")]
    ReceiveProtocolEventError(String),
    #[error("Protocol command event channel err:{0}")]
    SendProtocolCommandError(#[from] tokio::sync::mpsc::error::SendError<ProtocolCommand>),
    #[error("Protocol Node event channel err:{0}")]
    SendProtocolNodeError(#[from] tokio::sync::mpsc::error::SendError<NodeEvent>),
    #[error("Protocol Message channel err:{0}")]
    SendProtocolMessageError(#[from] tokio::sync::mpsc::error::SendError<Message>),
    #[error("Protocol NodeCommand channel err:{0}")]
    SendProtocolNodeCommandError(#[from] tokio::sync::mpsc::error::SendError<NodeCommand>),
    #[error("Failed retrieving network controller event")]
    NetworkControllerEventError,
    #[error("Network command channel error: {0}")]
    SendNetworkCommandError(#[from] tokio::sync::mpsc::error::SendError<NetworkCommand>),
    #[error("Oneshot response error : {0}")]
    ResponseReceiveError(#[from] tokio::sync::oneshot::error::RecvError),
    #[error("node event receiver died")]
    NodeEventReceiverError,
    #[error("node event sender died")]
    NodeEventSenderError,
    #[error("controller event receiver died")]
    ControllerEventReceiverError,
    #[error("protocol event channel error {0}")]
    SendProtocolEventError(#[from] tokio::sync::mpsc::error::SendError<ProtocolEvent>),
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
