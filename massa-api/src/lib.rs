//! Copyright (c) 2022 MASSA LABS <info@massa.net>
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
    AddressInfo, BlockInfo, BlockSummary, DatastoreEntryInput, DatastoreEntryOutput,
    EndorsementInfo, EventFilter, NodeStatus, OperationInfo, OperationInput,
    ReadOnlyBytecodeExecution, ReadOnlyCall, TimeInterval,
};
use massa_models::clique::Clique;
use massa_models::composite::PubkeySig;
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::node::NodeId;
use massa_models::operation::OperationId;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::PreHashSet;
use massa_models::{
    address::Address,
    block::{Block, BlockId},
    endorsement::EndorsementId,
    slot::Slot,
    version::Version,
};
use massa_network_exports::{NetworkCommandSender, NetworkConfig};
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolCommandSender;
use massa_storage::Storage;
use massa_wallet::Wallet;
use parking_lot::RwLock;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use tokio::sync::mpsc;
use tracing::{info, warn};

mod config;
mod error;
mod private;
mod public;
pub use config::APIConfig;

/// Public API component
pub struct Public {
    /// link to the consensus component
    pub consensus_command_sender: ConsensusCommandSender,
    /// link to the execution component
    pub execution_controller: Box<dyn ExecutionController>,
    /// link to the selector component
    pub selector_controller: Box<dyn SelectorController>,
    /// link to the pool component
    pub pool_command_sender: Box<dyn PoolController>,
    /// link to the protocol component
    pub protocol_command_sender: ProtocolCommandSender,
    /// Massa storage
    pub storage: Storage,
    /// consensus configuration (TODO: remove it, can be retrieved via an endpoint)
    pub consensus_config: ConsensusConfig,
    /// API settings
    pub api_settings: APIConfig,
    /// network setting
    pub network_settings: NetworkConfig,
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
    pub api_settings: APIConfig,
    /// stop channel
    pub stop_node_channel: mpsc::Sender<()>,
    /// User wallet
    pub node_wallet: Arc<RwLock<Wallet>>,
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
    let thread_builder = thread::Builder::new().name("rpc-server".into());
    let join_handle = thread_builder
        .spawn(|| server.wait())
        .expect("failed to spawn thread : rpc-server");

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

    /// Add a vector of new secret(private) keys for the node to use to stake.
    /// No confirmation to expect.
    #[rpc(name = "add_staking_secret_keys")]
    fn add_staking_secret_keys(&self, _: Vec<String>) -> BoxFuture<Result<(), ApiError>>;

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
    fn get_staking_addresses(&self) -> BoxFuture<Result<PreHashSet<Address>, ApiError>>;

    /// Bans given IP address(es).
    /// No confirmation to expect.
    #[rpc(name = "node_ban_by_ip")]
    fn node_ban_by_ip(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>>;

    /// Bans given node id.
    /// No confirmation to expect.
    #[rpc(name = "node_ban_by_id")]
    fn node_ban_by_id(&self, _: Vec<NodeId>) -> BoxFuture<Result<(), ApiError>>;

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

    /// Unban given IP address(es).
    /// No confirmation to expect.
    #[rpc(name = "node_unban_by_ip")]
    fn node_unban_by_ip(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>>;

    /// Unban given node id.
    /// No confirmation to expect.
    #[rpc(name = "node_unban_by_id")]
    fn node_unban_by_id(&self, _: Vec<NodeId>) -> BoxFuture<Result<(), ApiError>>;

    /// Summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count.
    #[rpc(name = "get_status")]
    fn get_status(&self) -> BoxFuture<Result<NodeStatus, ApiError>>;

    /// Get cliques.
    #[rpc(name = "get_cliques")]
    fn get_cliques(&self) -> BoxFuture<Result<Vec<Clique>, ApiError>>;

    /// Returns the active stakers and their active roll counts for the current cycle.
    #[rpc(name = "get_stakers")]
    fn get_stakers(&self) -> BoxFuture<Result<Vec<(Address, u64)>, ApiError>>;

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

    /// Get information on the block at a slot in the blockclique.
    /// If there is no block at this slot a `None` is returned.
    #[rpc(name = "get_blockclique_block_by_slot")]
    fn get_blockclique_block_by_slot(&self, _: Slot) -> BoxFuture<Result<Option<Block>, ApiError>>;

    /// Get the block graph within the specified time interval.
    /// Optional parameters: from `<time_start>` (included) and to `<time_end>` (excluded) millisecond timestamp
    #[rpc(name = "get_graph_interval")]
    fn get_graph_interval(&self, _: TimeInterval)
        -> BoxFuture<Result<Vec<BlockSummary>, ApiError>>;

    /// Get multiple datastore entries.
    #[rpc(name = "get_datastore_entries")]
    fn get_datastore_entries(
        &self,
        _: Vec<DatastoreEntryInput>,
    ) -> BoxFuture<Result<Vec<DatastoreEntryOutput>, ApiError>>;

    /// Get addresses.
    #[rpc(name = "get_addresses")]
    fn get_addresses(&self, _: Vec<Address>) -> BoxFuture<Result<Vec<AddressInfo>, ApiError>>;

    /// Adds operations to pool. Returns operations that were ok and sent to pool.
    #[rpc(name = "send_operations")]
    fn send_operations(
        &self,
        _: Vec<OperationInput>,
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
