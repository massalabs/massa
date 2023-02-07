//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Bootstrap crate
//!
//! At start up, if now is after genesis timestamp,
//! the node will bootstrap from one of the provided bootstrap servers.
//!
//! On server side, the server will query consensus for the graph and the ledger,
//! execution for execution related data and network for the peer list.
//!
#![feature(async_closure)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(ip)]
#![feature(let_chains)]

pub use establisher::types::Establisher;
use massa_consensus_exports::bootstrapable_graph::BootstrapableGraph;
use massa_final_state::FinalState;
use massa_network_exports::BootstrapPeers;
use parking_lot::RwLock;
use std::sync::Arc;

mod client;
mod client_binder;
mod error;
mod establisher;
mod messages;
mod server;
mod server_binder;
mod settings;
mod tools;
pub use client::get_state;
pub use establisher::types;
pub use messages::{
    BootstrapClientMessage, BootstrapClientMessageDeserializer, BootstrapClientMessageSerializer,
    BootstrapServerMessage, BootstrapServerMessageDeserializer, BootstrapServerMessageSerializer,
};
pub use server::{start_bootstrap_server, BootstrapManager};
pub use settings::IpType;
pub use settings::{BootstrapConfig, BootstrapServerMessageDeserializerArgs};

#[cfg(test)]
pub mod tests;

/// a collection of the bootstrap state snapshots of all relevant modules
pub struct GlobalBootstrapState {
    /// state of the final state
    pub final_state: Arc<RwLock<FinalState>>,

    /// state of the consensus graph
    pub graph: Option<BootstrapableGraph>,

    /// list of network peers
    pub peers: Option<BootstrapPeers>,
}

impl GlobalBootstrapState {
    fn new(final_state: Arc<RwLock<FinalState>>) -> Self {
        Self {
            final_state,
            graph: None,
            peers: None,
        }
    }
}
