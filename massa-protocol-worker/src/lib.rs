// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Protocol component
//! High level management of communications between nodes

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(let_chains)]
#![feature(map_try_insert)]

use massa_models::operation::Operation;
use tokio::sync::broadcast::Sender;

/// protocol worker
pub mod protocol_worker;
pub mod worker_operations_impl;
pub use protocol_worker::start_protocol_controller;
mod cache;
mod checked_operations;
mod node_info;
mod protocol_network;
mod sig_verifier;

/// WebSocket configuration.
#[derive(Clone)]
pub struct WsConfig {
    /// Whether WebSockets are enabled
    pub enabled: bool,
    /// Broadcast sender(channel) for operations
    pub operation_sender: Sender<Operation>,
}

#[cfg(test)]
pub mod tests;
