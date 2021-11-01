// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(async_closure)]

//! # Massa JSON-RPC API
//!
//! This crate exposes Rust methods (through the [`Endpoints`] trait) as JSON-RPC API endpoints
//! (thanks to the [Parity JSON-RPC](https://github.com/paritytech/jsonrpc) crate).
//!
//! **E.g.** this curl command will call endpoint `stop_node` (and stop the locally running `massa-node`):
//!
//! ```bash
//! curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0", "method": "stop_node", "id": 123 }' 127.0.0.1:33034
//! ```
//!
//! Endpoints are organised in 2 authorisations levels:
//!
//! - **Public** API, a.k.a. _"user mode"_ endpoints (running by default on `[::]:33035`)
//!     - Explorer (aggregated stats)
//!         - [`Endpoints::get_status`] Summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count.
//!         - [`Endpoints::get_cliques`] Get cliques.
//!     - Debug (specific information)
//!         - [`Endpoints::get_algo_config`] Returns the configuration used by consensus algorithm.
//!         - [`Endpoints::get_compensation_millis`] Returns the compensation_millis.
//!         - [`Endpoints::get_stakers`] Returns the active stakers and their roll counts for the current cycle.
//!         - [`Endpoints::get_operations`] Returns operations information associated to a given list of operations' IDs.
//!         - [`Endpoints::get_endorsements`] Get endorsements (not yet implemented)
//!         - [`Endpoints::get_blocks`] Get information on a block given its hash.
//!         - [`Endpoints::get_graph_interval`] Get the block graph within the specified time interval.
//!         - [`Endpoints::get_addresses`] Get addresses.
//!     - User (interaction with the node)
//!         - [`Endpoints::send_operations`] Adds operations to pool. Returns operations that were ok and sent to pool.
//!
//! - **Private** API, a.k.a. _"manager mode"_ endpoints (running by default on `127.0.0.1:33034`)
//!     - [`Endpoints::stop_node`] Gracefully stop the node.
//!     - [`Endpoints::node_sign_message`] Sign message with node's key.
//!     - [`Endpoints::add_staking_private_keys`] Add a vec of new private keys for the node to use to stake.
//!     - [`Endpoints::remove_staking_addresses`] Remove a vec of addresses used to stake.
//!     - [`Endpoints::get_staking_addresses`] Return hashset of staking addresses.
//!     - [`Endpoints::ban`] Bans given IP address.
//!     - [`Endpoints::unban`] Unbans given IP address.

use consensus::{ConsensusCommandSender, ConsensusConfig};
use crypto::signature::PrivateKey;
use error::ApiError;
use jsonrpc_core::{BoxFuture, IoHandler};
use jsonrpc_derive::rpc;
use jsonrpc_http_server::{CloseHandle, ServerBuilder};
use models::address::{AddressHashMap, AddressHashSet};
use models::api::{
    APIConfig, AddressInfo, BlockInfo, BlockSummary, EndorsementInfo, NodeStatus, OperationInfo,
    RollsInfo, TimeInterval,
};
use models::clique::Clique;
use models::crypto::PubkeySig;
use models::node::NodeId;
use models::operation::{Operation, OperationId};
use models::{Address, AlgoConfig, BlockId, EndorsementId, Version};
use network::{NetworkCommandSender, NetworkConfig};
use pool::PoolCommandSender;
use std::net::{IpAddr, SocketAddr};
use std::thread;
use std::thread::JoinHandle;
use storage::StorageAccess;
use tokio::sync::mpsc;
use tracing::{info, warn};

mod error;
mod private;
mod public;

pub struct Public {
    pub consensus_command_sender: ConsensusCommandSender,
    pub pool_command_sender: PoolCommandSender,
    pub storage_command_sender: StorageAccess,
    pub consensus_config: ConsensusConfig,
    pub api_config: APIConfig,
    pub network_config: NetworkConfig,
    pub version: Version,
    pub network_command_sender: NetworkCommandSender,
    pub compensation_millis: i64,
    pub node_id: NodeId,
}

pub struct Private {
    pub consensus_command_sender: ConsensusCommandSender,
    pub network_command_sender: NetworkCommandSender,
    pub consensus_config: ConsensusConfig,
    pub api_config: APIConfig,
    pub stop_node_channel: mpsc::Sender<()>,
}

pub struct API<T>(T);

pub trait RpcServer: Endpoints {
    fn serve(self, _: &SocketAddr) -> StopHandle;
}

fn serve(api: impl Endpoints, url: &SocketAddr) -> StopHandle {
    let mut io = IoHandler::new();
    io.extend_with(api.to_delegate());

    let server = ServerBuilder::new(io)
        .event_loop_executor(tokio::runtime::Handle::current())
        .start_http(url)
        .expect("Unable to start RPC server");

    let close_handle = server.close_handle();
    let join_handle = thread::spawn(|| server.wait());

    StopHandle {
        close_handle,
        join_handle,
    }
}

pub struct StopHandle {
    close_handle: CloseHandle,
    join_handle: JoinHandle<()>,
}

impl StopHandle {
    pub fn stop(self) {
        self.close_handle.close();
        if let Err(err) = self.join_handle.join() {
            warn!("API thread panicked: {:?}", err);
        } else {
            info!("API finished cleanly");
        }
    }
}

#[rpc(server)]
pub trait Endpoints {
    /// Gracefully stop the node.
    #[rpc(name = "stop_node")]
    fn stop_node(&self) -> BoxFuture<Result<(), ApiError>>;

    /// Sign message with node's key.
    /// Returns the public key that signed the message and the signature.
    #[rpc(name = "node_sign_message")]
    fn node_sign_message(&self, _: Vec<u8>) -> BoxFuture<Result<PubkeySig, ApiError>>;

    /// Add a vec of new private keys for the node to use to stake.
    /// No confirmation to expect.
    #[rpc(name = "add_staking_private_keys")]
    fn add_staking_private_keys(&self, _: Vec<PrivateKey>) -> BoxFuture<Result<(), ApiError>>;

    /// Remove a vec of addresses used to stake.
    /// No confirmation to expect.
    #[rpc(name = "remove_staking_addresses")]
    fn remove_staking_addresses(&self, _: Vec<Address>) -> BoxFuture<Result<(), ApiError>>;

    /// Return hashset of staking addresses.
    #[rpc(name = "get_staking_addresses")]
    fn get_staking_addresses(&self) -> BoxFuture<Result<AddressHashSet, ApiError>>;

    /// Bans given IP address.
    /// No confirmation to expect.
    #[rpc(name = "ban")]
    fn ban(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>>;

    /// Unbans given IP address.
    /// No confirmation to expect.
    #[rpc(name = "unban")]
    fn unban(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>>;

    /// Summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count.
    #[rpc(name = "get_status")]
    fn get_status(&self) -> BoxFuture<Result<NodeStatus, ApiError>>;

    /// Get cliques.
    #[rpc(name = "get_cliques")]
    fn get_cliques(&self) -> BoxFuture<Result<Vec<Clique>, ApiError>>;

    /// Returns the configuration used by consensus algorithm.
    #[rpc(name = "get_algo_config")]
    fn get_algo_config(&self) -> BoxFuture<Result<AlgoConfig, ApiError>>;

    /// Returns the compensation_millis.
    #[rpc(name = "get_compensation_millis")]
    fn get_compensation_millis(&self) -> BoxFuture<Result<i64, ApiError>>;

    /// Returns the active stakers and their roll counts for the current cycle.
    #[rpc(name = "get_stakers")]
    fn get_stakers(&self) -> BoxFuture<Result<AddressHashMap<RollsInfo>, ApiError>>;

    /// Returns operations information associated to a given list of operations' IDs.
    #[rpc(name = "get_operations")]
    fn get_operations(
        &self,
        _: Vec<OperationId>,
    ) -> BoxFuture<Result<Vec<OperationInfo>, ApiError>>;

    /// Get endorsements (not yet implemented).
    #[rpc(name = "get_endorsements")]
    fn get_endorsements(
        &self,
        _: Vec<EndorsementId>,
    ) -> BoxFuture<Result<Vec<EndorsementInfo>, ApiError>>;

    /// Get information on a block given its hash.
    #[rpc(name = "get_blocks")]
    fn get_blocks(&self, _: Vec<BlockId>) -> BoxFuture<Result<Vec<BlockInfo>, ApiError>>;

    /// Get the block graph within the specified time interval.
    /// Optional parameters: from `<time_start>` (included) and to `<time_end>` (excluded) millisecond timestamp
    #[rpc(name = "get_graph_interval")]
    fn get_graph_interval(&self, _: TimeInterval)
        -> BoxFuture<Result<Vec<BlockSummary>, ApiError>>;

    /// Get addresses.
    #[rpc(name = "get_addresses")]
    fn get_addresses(&self, _: Vec<Address>) -> BoxFuture<Result<Vec<AddressInfo>, ApiError>>;

    /// Adds operations to pool. Returns operations that were ok and sent to pool.
    #[rpc(name = "send_operations")]
    fn send_operations(&self, _: Vec<Operation>) -> BoxFuture<Result<Vec<OperationId>, ApiError>>;
}
