mod binders;
mod common;
mod config;
pub mod handshake_worker;
mod messages;
mod node_worker;
mod protocol_controller;
mod protocol_worker;

pub use common::NodeId;
pub use config::ProtocolConfig;
pub use protocol_controller::{
    start_protocol_controller, ProtocolCommandSender, ProtocolEventReceiver, ProtocolManager,
};
pub use protocol_worker::{ProtocolCommand, ProtocolEvent};

#[cfg(test)]
pub mod tests;
