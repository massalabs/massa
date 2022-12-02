//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Json RPC API for a massa-node
#![feature(async_closure)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
use crate::error::ApiError::WrongAPI;
use hyper::Method;
use jsonrpsee::core::{Error as JsonRpseeError, RpcResult};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::{AllowHosts, ServerBuilder, ServerHandle};
use massa_consensus_exports::ConsensusController;
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
use serde_json::Value;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

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
    pub consensus_controller: Box<dyn ConsensusController>,
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
    /// link to the network component
    pub network_command_sender: NetworkCommandSender,
    /// link to the execution component
    pub execution_controller: Box<dyn ExecutionController>,
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
#[async_trait::async_trait]
pub trait RpcServer: MassaRpcServer {
    /// Start the API
    async fn serve(
        self,
        url: &SocketAddr,
        api_config: &APIConfig,
    ) -> Result<StopHandle, JsonRpseeError>;
}

async fn serve(
    api: impl MassaRpcServer,
    url: &SocketAddr,
    api_config: &APIConfig,
) -> Result<StopHandle, JsonRpseeError> {
    let allowed_hosts = if api_config.allow_hosts.is_empty() {
        AllowHosts::Any
    } else {
        let hosts = api_config
            .allow_hosts
            .iter()
            .map(|hostname| hostname.into())
            .collect();
        AllowHosts::Only(hosts)
    };

    let mut server_builder = ServerBuilder::new()
        .max_request_body_size(api_config.max_request_body_size)
        .max_response_body_size(api_config.max_response_body_size)
        .max_connections(api_config.max_connections)
        .set_host_filtering(allowed_hosts)
        .batch_requests_supported(api_config.batch_requests_supported)
        .ping_interval(api_config.ping_interval.to_duration());

    if api_config.enable_http && !api_config.enable_ws {
        server_builder = server_builder.http_only();
    } else if api_config.enable_ws && !api_config.enable_http {
        server_builder = server_builder.ws_only()
    } else {
        panic!("wrong server configuration, you can't disable both http and ws")
    }

    let cors = CorsLayer::new()
        // Allow `POST` and `OPTIONS` when accessing the resource
        .allow_methods([Method::POST, Method::OPTIONS])
        // Allow requests from any origin
        .allow_origin(Any)
        .allow_headers([hyper::header::CONTENT_TYPE]);

    let middleware = tower::ServiceBuilder::new().layer(cors);

    let server = server_builder
        .set_middleware(middleware)
        .build(url)
        .await
        .expect("failed to build server");

    let server_handler = server.start(api.into_rpc()).expect("server start failed");
    let stop_handler = StopHandle { server_handler };

    Ok(stop_handler)
}

/// Used to be able to stop the API
pub struct StopHandle {
    server_handler: ServerHandle,
}

impl StopHandle {
    /// stop the API gracefully
    pub fn stop(self) {
        match self.server_handler.stop() {
            Ok(_) => {
                info!("API finished cleanly");
            }
            Err(err) => warn!("API thread panicked: {:?}", err),
        }
    }
}

/// Exposed API methods
#[rpc(server)]
pub trait MassaRpc {
    /// Gracefully stop the node.
    #[method(name = "stop_node")]
    async fn stop_node(&self) -> RpcResult<()>;

    /// Sign message with node's key.
    /// Returns the public key that signed the message and the signature.
    #[method(name = "node_sign_message")]
    async fn node_sign_message(&self, arg: Vec<u8>) -> RpcResult<PubkeySig>;

    /// Add a vector of new secret(private) keys for the node to use to stake.
    /// No confirmation to expect.
    #[method(name = "add_staking_secret_keys")]
    async fn add_staking_secret_keys(&self, arg: Vec<String>) -> RpcResult<()>;

    /// Execute bytecode in read-only mode.
    #[method(name = "execute_read_only_bytecode")]
    async fn execute_read_only_bytecode(
        &self,
        arg: Vec<ReadOnlyBytecodeExecution>,
    ) -> RpcResult<Vec<ExecuteReadOnlyResponse>>;

    /// Execute an SC function in read-only mode.
    #[method(name = "execute_read_only_call")]
    async fn execute_read_only_call(
        &self,
        arg: Vec<ReadOnlyCall>,
    ) -> RpcResult<Vec<ExecuteReadOnlyResponse>>;

    /// Remove a vector of addresses used to stake.
    /// No confirmation to expect.
    #[method(name = "remove_staking_addresses")]
    async fn remove_staking_addresses(&self, arg: Vec<Address>) -> RpcResult<()>;

    /// Return hash set of staking addresses.
    #[method(name = "get_staking_addresses")]
    async fn get_staking_addresses(&self) -> RpcResult<PreHashSet<Address>>;

    /// Bans given IP address(es).
    /// No confirmation to expect.
    #[method(name = "node_ban_by_ip")]
    async fn node_ban_by_ip(&self, arg: Vec<IpAddr>) -> RpcResult<()>;

    /// Bans given node id.
    /// No confirmation to expect.
    #[method(name = "node_ban_by_id")]
    async fn node_ban_by_id(&self, arg: Vec<NodeId>) -> RpcResult<()>;

    /// whitelist given IP address.
    /// No confirmation to expect.
    /// Note: If the ip was unknown it adds it to the known peers, otherwise it updates the peer type
    #[method(name = "node_whitelist")]
    async fn node_whitelist(&self, arg: Vec<IpAddr>) -> RpcResult<()>;

    /// remove from whitelist given IP address.
    /// keep it as standard
    /// No confirmation to expect.
    #[method(name = "node_remove_from_whitelist")]
    async fn node_remove_from_whitelist(&self, arg: Vec<IpAddr>) -> RpcResult<()>;

    /// Unban given IP address(es).
    /// No confirmation to expect.
    #[method(name = "node_unban_by_ip")]
    async fn node_unban_by_ip(&self, arg: Vec<IpAddr>) -> RpcResult<()>;

    /// Unban given node id.
    /// No confirmation to expect.
    #[method(name = "node_unban_by_id")]
    async fn node_unban_by_id(&self, arg: Vec<NodeId>) -> RpcResult<()>;

    /// Summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count.
    #[method(name = "get_status")]
    async fn get_status(&self) -> RpcResult<NodeStatus>;

    /// Get cliques.
    #[method(name = "get_cliques")]
    async fn get_cliques(&self) -> RpcResult<Vec<Clique>>;

    /// Returns the active stakers and their active roll counts for the current cycle.
    #[method(name = "get_stakers")]
    async fn get_stakers(&self) -> RpcResult<Vec<(Address, u64)>>;

    /// Returns operations information associated to a given list of operations' IDs.
    #[method(name = "get_operations")]
    async fn get_operations(&self, arg: Vec<OperationId>) -> RpcResult<Vec<OperationInfo>>;

    /// Get endorsements (not yet implemented).
    #[method(name = "get_endorsements")]
    async fn get_endorsements(&self, arg: Vec<EndorsementId>) -> RpcResult<Vec<EndorsementInfo>>;

    /// Get information on a block given its hash.
    #[method(name = "get_block")]
    async fn get_block(&self, arg: BlockId) -> RpcResult<BlockInfo>;

    /// Get information on the block at a slot in the blockclique.
    /// If there is no block at this slot a `None` is returned.
    #[method(name = "get_blockclique_block_by_slot")]
    async fn get_blockclique_block_by_slot(&self, arg: Slot) -> RpcResult<Option<Block>>;

    /// Get the block graph within the specified time interval.
    /// Optional parameters: from `<time_start>` (included) and to `<time_end>` (excluded) millisecond timestamp
    #[method(name = "get_graph_interval")]
    async fn get_graph_interval(&self, arg: TimeInterval) -> RpcResult<Vec<BlockSummary>>;

    /// Get multiple datastore entries.
    #[method(name = "get_datastore_entries")]
    async fn get_datastore_entries(
        &self,
        arg: Vec<DatastoreEntryInput>,
    ) -> RpcResult<Vec<DatastoreEntryOutput>>;

    /// Get addresses.
    #[method(name = "get_addresses")]
    async fn get_addresses(&self, arg: Vec<Address>) -> RpcResult<Vec<AddressInfo>>;

    /// Adds operations to pool. Returns operations that were ok and sent to pool.
    #[method(name = "send_operations")]
    async fn send_operations(&self, arg: Vec<OperationInput>) -> RpcResult<Vec<OperationId>>;

    /// Get events optionally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    #[method(name = "get_filtered_sc_output_event")]
    async fn get_filtered_sc_output_event(&self, arg: EventFilter)
        -> RpcResult<Vec<SCOutputEvent>>;

    /// Get OpenRPC specification.
    #[method(name = "rpc.discover")]
    async fn get_openrpc_spec(&self) -> RpcResult<Value>;
}

fn wrong_api<T>() -> RpcResult<T> {
    Err((WrongAPI).into())
}

fn _jsonrpsee_assert(_method: &str, _request: Value, _response: Value) {
    // TODO: jsonrpsee_client_transports::RawClient::call_method ... see #1182
}
