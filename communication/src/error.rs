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
    #[error("A tokio task has crashed err:{0}")]
    TokioTaskJoinError(#[from] tokio::task::JoinError),
    #[error("PeerInfo Not found error for ip:`{0}`")]
    PeerInfoNotFoundError(IpAddr),
    #[error("Try to close a connection with no connection open error for ip:`{0}`")]
    ToManyActiveConnectionClosed(IpAddr),
    #[error(
        "Try to open a connection with too many out connection attempts done error for ip:`{0}`"
    )]
    ToManyConnectionAttempt(IpAddr),
    #[error("Try to close a out connection failure error for ip:`{0}`")]
    ToManyConnectionFailure(IpAddr),
    #[error("The ip:`{0}` address is not global")]
    TargetIpIsNotGLobal(IpAddr),
    #[error("Active connection missing:`{0}`")]
    ActiveConnectionMissing(ConnectionId),
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
    #[error("expect that the id is not already in running_handshakes id:{0}")]
    HandshakeIdAlreadyExistError(String),
    #[error("Protocol err:{0}")]
    GeneralProtocolError(String),
    #[error("Failed retrieving network controller event")]
    NetworkControllerEventError,
    #[error("Network command channel error: {0}")]
    SendNetworkCommandError(#[from] tokio::sync::mpsc::error::SendError<NetworkCommand>),
    #[error("Oneshot response error : {0}")]
    ResponseReceiveError(#[from] tokio::sync::oneshot::error::RecvError),
    #[error("Mock error {0}")]
    MockError(String),
    #[error("Event from missign node")]
    MissingNodeError,
    #[error("config routable_ip IP is not routable")]
    ConfigRoutableIpError,
    #[error("IO error : {0}")]
    IOError(#[from] std::io::Error),
    #[error("Serde error : {0}")]
    SerdeError(#[from] serde_json::Error),
    #[error("node event receiver died")]
    NodeEventReceiverError,
    #[error("node event sender died")]
    NodeEventSenderError,
    #[error("controller event receiver died")]
    ControllerEventReceiverError,
    #[error("protocol event channel error {0}")]
    SendProtocolEventError(#[from] tokio::sync::mpsc::error::SendError<ProtocolEvent>),
    #[error("crypto error {0}")]
    CryptoError(#[from] crypto::CryptoError),
    #[error("Flexbuffer serialisation error {0}")]
    FlexbufferSerializationError(#[from] flexbuffers::SerializationError),
    #[error("Flexbuffer reader error {0}")]
    FlexbufferReaderError(#[from] flexbuffers::ReaderError),
    #[error("Flexbuffer deserialisation error {0}")]
    FlexbufferDeserializationError(#[from] flexbuffers::DeserializationError),
    #[error("handhsake timed out")]
    HandshakeTimeoutError,
    #[error("handshake interrupted")]
    HandshakeInterruptionError,
    #[error("handshake wrong message")]
    HandshakeWrongMessageError,
    #[error("handshake announced own public key")]
    HandshakeKeyError,
    #[error("handshake remote signature invalid")]
    HandshakeInvalidSignatureError,
    #[error("the protocol controller should have close everything before shuting down")]
    UnexpectedProtocolControllerClosureError,
    #[error("Time error {0}")]
    TimeError(#[from] time::TimeError),
}
