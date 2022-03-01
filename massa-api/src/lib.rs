// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![feature(async_closure)]

use crate::error::ApiError::WrongAPI;
use error::ApiError;
use jsonrpc_core::{BoxFuture, IoHandler, Value};
use jsonrpc_derive::rpc;
use jsonrpc_http_server::{CloseHandle, ServerBuilder};
use massa_consensus_exports::{ConsensusCommandSender, ConsensusConfig};
use massa_execution_exports::ExecutionController;
use massa_models::api::{
    APISettings, AddressInfo, BlockInfo, BlockSummary, EndorsementInfo, EventFilter, NodeStatus,
    OperationInfo, ReadOnlyExecution, TimeInterval,
};
use massa_models::clique::Clique;
use massa_models::composite::PubkeySig;
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::node::NodeId;
use massa_models::operation::{Operation, OperationId};
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::{Map, Set};
use massa_models::{Address, BlockId, EndorsementId, Version};
use massa_network::{NetworkCommandSender, NetworkSettings};
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

pub struct Public {
    pub consensus_command_sender: ConsensusCommandSender,
    pub execution_controller: Box<dyn ExecutionController>,
    pub pool_command_sender: PoolCommandSender,
    pub consensus_config: ConsensusConfig,
    pub api_settings: &'static APISettings,
    pub network_settings: &'static NetworkSettings,
    pub version: Version,
    pub network_command_sender: NetworkCommandSender,
    pub compensation_millis: i64,
    pub node_id: NodeId,
}

pub struct Private {
    pub consensus_command_sender: ConsensusCommandSender,
    pub network_command_sender: NetworkCommandSender,
    pub execution_controller: Box<dyn ExecutionController>,
    pub consensus_config: ConsensusConfig,
    pub api_settings: &'static APISettings,
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

    /// Execute code in read-only mode.
    #[rpc(name = "execute_read_only_request")]
    fn execute_read_only_request(
        &self,
        _: Vec<ReadOnlyExecution>,
    ) -> BoxFuture<Result<Vec<ExecuteReadOnlyResponse>, ApiError>>;

    /// Remove a vec of addresses used to stake.
    /// No confirmation to expect.
    #[rpc(name = "remove_staking_addresses")]
    fn remove_staking_addresses(&self, _: Vec<Address>) -> BoxFuture<Result<(), ApiError>>;

    /// Return hashset of staking addresses.
    #[rpc(name = "get_staking_addresses")]
    fn get_staking_addresses(&self) -> BoxFuture<Result<Set<Address>, ApiError>>;

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
    fn send_operations(&self, _: Vec<Operation>) -> BoxFuture<Result<Vec<OperationId>, ApiError>>;

    /// Get events optionnally filtered by:
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
