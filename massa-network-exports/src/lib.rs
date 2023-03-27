//! Manages a connection with a node

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(ip)]

pub use commands::{
    AskForBlocksInfo, BlockInfoReply, NetworkCommand, NetworkEvent, NetworkManagementCommand,
    NodeCommand, NodeEvent, NodeEventType,
};

pub use common::{ConnectionClosureReason, ConnectionId};
pub use error::{HandshakeErrorType, NetworkConnectionErrorType, NetworkError};
pub use establisher::{Establisher, Listener, ReadHalf, WriteHalf};
pub use network_controller::{
    MockNetworkCommandSender, NetworkCommandSender, NetworkCommandSenderTrait,
    NetworkEventReceiver, NetworkManager,
};
pub use peers::{
    BootstrapPeers, BootstrapPeersDeserializer, BootstrapPeersSerializer, ConnectionCount, Peer,
    PeerInfo, PeerType, Peers,
};
pub use settings::NetworkConfig;

mod commands;
mod common;
mod error;
mod establisher;
mod network_controller;
mod peers;

/// network settings
pub mod settings;

#[cfg(feature = "testing")]
/// test exports
pub mod test_exports;
