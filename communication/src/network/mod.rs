// Copyright (c) 2021 MASSA LABS <info@massa.net>

//! Manages a connection with a node
pub use common::{ConnectionClosureReason, ConnectionId};
pub use config::NetworkConfig;
pub use establisher::Establisher;
pub use establisher::*;
pub use models::stats::NetworkStats;
pub use network_controller::{
    start_network_controller, NetworkCommandSender, NetworkEventReceiver, NetworkManager,
};
pub use network_worker::{BootstrapPeers, NetworkCommand, NetworkEvent, Peer, Peers};
pub use peer_info_database::PeerInfo;

mod binders;
mod common;
mod config;
mod establisher;
mod handshake_worker;
mod messages;
mod network_controller;
mod network_worker;
mod node_worker;
mod peer_info_database;

#[cfg(test)]
pub mod tests;
