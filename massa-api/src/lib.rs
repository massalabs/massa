// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Json RPC API for a massa-node
#![feature(async_closure)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
use crate::error::ApiError::WrongAPI;
use error::ApiError;
use jsonrpc_core::{BoxFuture, IoHandler, Value};
use jsonrpc_derive::rpc;
use jsonrpc_http_server::{CloseHandle, ServerBuilder};
use massa_consensus_exports::{ConsensusCommandSender, ConsensusConfig};
use massa_execution_exports::ExecutionController;
use massa_models::api::{
    AddressInfo, BlockInfo, BlockSummary, EndorsementInfo, EventFilter, NodeStatus, OperationInfo,
    ReadOnlyBytecodeExecution, ReadOnlyCall, TimeInterval,
};
use massa_models::clique::Clique;
use massa_models::composite::PubkeySig;
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::node::NodeId;
use massa_models::operation::OperationId;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::{Map, Set};
use massa_models::{Address, BlockId, EndorsementId, SignedOperation, Version};
use massa_network_exports::{NetworkCommandSender, NetworkSettings};
use massa_pool::PoolCommandSender;
use massa_signature::PrivateKey;
use std::net::{IpAddr, SocketAddr};
use std::thread;
use std::thread::JoinHandle;
use tokio::sync::mpsc;
use tracing::{info, warn};

mod error;
mod private;
mod public;
mod settings;
pub use settings::APISettings;

/// Public API component
pub struct Public {
    /// link to the consensus component
    pub consensus_command_sender: ConsensusCommandSender,
    /// link to the execution component
    pub execution_controller: Box<dyn ExecutionController>,
    /// link to the pool component
    pub pool_command_sender: PoolCommandSender,
    /// consensus configuration (TODO: remove it, can be retrieved via an endpoint)
    pub consensus_config: ConsensusConfig,
    /// API settings
    pub api_settings: &'static APISettings,
    /// network setting (TODO consider removing)
    pub network_settings: &'static NetworkSettings,
    /// node version (TODO remove, can be retrieved via an endpoint)
    pub version: Version,
    /// link to the network component
    pub network_command_sender: NetworkCommandSender,
    /// compensation milliseconds (used to sync time with bootstrap server)
    pub compensation_millis: i64,
    /// our node id
    pub node_id: NodeId,
}

/// Private API content
pub struct Private {
    /// link to the consensus component
    pub consensus_command_sender: ConsensusCommandSender,
    /// link to the network component
    pub network_command_sender: NetworkCommandSender,
    /// link to the execution component
    pub execution_controller: Box<dyn ExecutionController>,
    /// consensus configuration (TODO: remove it, can be retrieved via an endpoint)
    pub consensus_config: ConsensusConfig,
    /// API settings
    pub api_settings: &'static APISettings,
    /// stop channel
    pub stop_node_channel: mpsc::Sender<()>,
}

/// The API wrapper
pub struct API<T>(T);

/// Used to manage the API
pub trait RpcServer: Endpoints {
    /// Start the API
    fn serve(self, _: &SocketAddr) -> StopHandle;
}

fn serve(api: impl Endpoints, url: &SocketAddr) -> StopHandle {
    let mut io = IoHandler::new();
    io.extend_with(api.to_delegate());

    let server = ServerBuilder::new(io)
        .event_loop_executor(tokio::runtime::Handle::current())
        .max_request_body_size(50 * 1024 * 1024)
        .start_http(url)
        .expect("Unable to start RPC server");

    let close_handle = server.close_handle();
    let join_handle = thread::spawn(|| server.wait());

    StopHandle {
        close_handle,
        join_handle,
    }
}

/// Used to be able to stop the API
pub struct StopHandle {
    close_handle: CloseHandle,
    join_handle: JoinHandle<()>,
}

impl StopHandle {
    /// stop the API gracefully
    pub fn stop(self) {
        self.close_handle.close();
        if let Err(err) = self.join_handle.join() {
            warn!("API thread panicked: {:?}", err);
        } else {
            info!("API finished cleanly");
        }
    }
}

/// Exposed API endpoints
#[rpc(server)]
pub trait Endpoints {
    /// Gracefully stop the node.
    #[rpc(name = "stop_node")]
    fn stop_node(&self) -> BoxFuture<Result<(), ApiError>>;

    /// Sign message with node's key.
    /// Returns the public key that signed the message and the signature.
    #[rpc(name = "node_sign_message")]
    fn node_sign_message(&self, _: Vec<u8>) -> BoxFuture<Result<PubkeySig, ApiError>>;

    /// Add a vector of new private keys for the node to use to stake.
    /// No confirmation to expect.
    #[rpc(name = "add_staking_private_keys")]
    fn add_staking_private_keys(&self, _: Vec<PrivateKey>) -> BoxFuture<Result<(), ApiError>>;

    /// Execute bytecode in read-only mode.
    #[rpc(name = "execute_read_only_bytecode")]
    fn execute_read_only_bytecode(
        &self,
        _: Vec<ReadOnlyBytecodeExecution>,
    ) -> BoxFuture<Result<Vec<ExecuteReadOnlyResponse>, ApiError>>;

    /// Execute an SC function in read-only mode.
    #[rpc(name = "execute_read_only_call")]
    fn execute_read_only_call(
        &self,
        _: Vec<ReadOnlyCall>,
    ) -> BoxFuture<Result<Vec<ExecuteReadOnlyResponse>, ApiError>>;

    /// Remove a vector of addresses used to stake.
    /// No confirmation to expect.
    #[rpc(name = "remove_staking_addresses")]
    fn remove_staking_addresses(&self, _: Vec<Address>) -> BoxFuture<Result<(), ApiError>>;

    /// Return hash set of staking addresses.
    #[rpc(name = "get_staking_addresses")]
    fn get_staking_addresses(&self) -> BoxFuture<Result<Set<Address>, ApiError>>;

    /// Bans given IP address.
    /// No confirmation to expect.
    #[rpc(name = "ban")]
    fn ban(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>>;

    /// whitelist given IP address.
    /// No confirmation to expect.
    /// Note: If the ip was unknown it adds it to the known peers, otherwise it updates the peer type
    #[rpc(name = "node_whitelist")]
    fn node_whitelist(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>>;

    /// remove from whitelist given IP address.
    /// keep it as standard
    /// No confirmation to expect.
    #[rpc(name = "node_remove_from_whitelist")]
    fn node_remove_from_whitelist(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>>;

    /// Unban given IP address.
    /// No confirmation to expect.
    #[rpc(name = "unban")]
    fn unban(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>>;

    /// Summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count.
    #[rpc(name = "get_status")]
    fn get_status(&self) -> BoxFuture<Result<NodeStatus, ApiError>>;

    /// Get cliques.
    #[rpc(name = "get_cliques")]
    fn get_cliques(&self) -> BoxFuture<Result<Vec<Clique>, ApiError>>;

    /// Returns the active stakers and their active roll counts for the current cycle.
    #[rpc(name = "get_stakers")]
    fn get_stakers(&self) -> BoxFuture<Result<Map<Address, u64>, ApiError>>;

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
    #[rpc(name = "get_block")]
    fn get_block(&self, _: BlockId) -> BoxFuture<Result<BlockInfo, ApiError>>;

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
    fn send_operations(
        &self,
        _: Vec<SignedOperation>,
    ) -> BoxFuture<Result<Vec<OperationId>, ApiError>>;

    /// Get events optionally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    #[rpc(name = "get_filtered_sc_output_event")]
    fn get_filtered_sc_output_event(
        &self,
        _: EventFilter,
    ) -> BoxFuture<Result<Vec<SCOutputEvent>, ApiError>>;
}

fn wrong_api<T>() -> BoxFuture<Result<T, ApiError>> {
    let closure = async move || Err(WrongAPI);
    Box::pin(closure())
}

fn _jsonrpc_assert(_method: &str, _request: Value, _response: Value) {
    // TODO: jsonrpc_client_transports::RawClient::call_method ... see #1182
}
