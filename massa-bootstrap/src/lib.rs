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
pub use client::get_state;
pub use establisher::types;
pub use messages::{
    BootstrapClientMessage, BootstrapClientMessageDeserializer, BootstrapClientMessageSerializer,
    BootstrapServerMessage, BootstrapServerMessageDeserializer, BootstrapServerMessageSerializer,
};
pub use server::{start_bootstrap_server, BootstrapManager};
pub use settings::BootstrapConfig;

#[cfg(test)]
pub mod tests;

/// a collection of the bootstrap state snapshots of all relevant modules
pub struct GlobalBootstrapState {
    /// state of the consensus graph
    pub graph: Option<BootstrapableGraph>,

    /// timestamp correction in milliseconds
    pub compensation_millis: i64,

    /// list of network peers
    pub peers: Option<BootstrapPeers>,

    /// state of the final state
    pub final_state: Arc<RwLock<FinalState>>,
}

impl GlobalBootstrapState {
    fn new(final_state: Arc<RwLock<FinalState>>) -> Self {
        Self {
            graph: None,
            compensation_millis: Default::default(),
            peers: None,
            final_state,
        }
    }
}
