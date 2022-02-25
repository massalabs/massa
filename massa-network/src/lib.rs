// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]

//! Manages a connection with a node
pub use common::{ConnectionClosureReason, ConnectionId};
pub use error::NetworkError;
pub use establisher::Establisher;
pub use establisher::*;
pub use network_controller::{
    start_network_controller, NetworkCommandSender, NetworkEventReceiver, NetworkManager,
};
pub use network_worker::{BootstrapPeers, NetworkCommand, NetworkEvent, Peer, Peers};
pub use peer_info_database::PeerInfo;
pub use settings::NetworkSettings;

mod binders;
mod common;
mod error;
mod establisher;
mod handshake_worker;
mod messages;
mod network_controller;
mod network_worker;
mod node_worker;
mod peer_info_database;
pub mod settings;

#[cfg(test)]
pub mod tests;
