//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Json RPC API for a massa-node
#![feature(async_closure)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
use api_trait::MassaApiServer;
use hyper::Method;
use jsonrpsee::core::{Error as JsonRpseeError, RpcResult};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::{AllowHosts, BatchRequestConfig, ServerBuilder, ServerHandle};
use jsonrpsee::RpcModule;
use massa_api_exports::{
    address::AddressInfo,
    block::{BlockInfo, BlockSummary},
    config::APIConfig,
    datastore::{DatastoreEntryInput, DatastoreEntryOutput},
    endorsement::EndorsementInfo,
    error::ApiError::WrongAPI,
    execution::{ExecuteReadOnlyResponse, ReadOnlyBytecodeExecution, ReadOnlyCall},
    node::NodeStatus,
    operation::{OperationInfo, OperationInput},
    page::{PageRequest, PagedVec},
    TimeInterval,
};
use massa_consensus_exports::{ConsensusChannels, ConsensusController};
use massa_execution_exports::ExecutionController;
use massa_models::clique::Clique;
use massa_models::composite::PubkeySig;
use massa_models::node::NodeId;
use massa_models::operation::OperationId;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::PreHashSet;
use massa_models::{
    address::Address, block::Block, block_id::BlockId, endorsement::EndorsementId,
    execution::EventFilter, slot::Slot, version::Version,
};
use massa_pool_exports::{PoolChannels, PoolController};
use massa_pos_exports::SelectorController;
use massa_protocol_exports::{ProtocolConfig, ProtocolController};
use massa_storage::Storage;
use massa_versioning::keypair_factory::KeyPairFactory;
use massa_wallet::Wallet;
use parking_lot::RwLock;
use serde_json::Value;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Condvar, Mutex};
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn};

mod api;
mod api_trait;
mod private;
mod public;

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
    pub protocol_controller: Box<dyn ProtocolController>,
    /// Massa storage
    pub storage: Storage,
    /// API settings
    pub api_settings: APIConfig,
    /// Protocol config
    pub protocol_config: ProtocolConfig,
    /// node version (TODO remove, can be retrieved via an endpoint)
    pub version: Version,
    /// our node id
    pub node_id: NodeId,
    /// keypair factory
    pub keypair_factory: KeyPairFactory,
}

/// Private API content
pub struct Private {
    /// link to the protocol component
    pub protocol_controller: Box<dyn ProtocolController>,
    /// link to the execution component
    pub execution_controller: Box<dyn ExecutionController>,
    /// API settings
    pub api_settings: APIConfig,
    /// Condition-variable pair to stop the system
    pub stop_cv: Arc<(Mutex<bool>, Condvar)>,
    /// User wallet
    pub node_wallet: Arc<RwLock<Wallet>>,
}

/// API v2 content
pub struct ApiV2 {
    /// link to the consensus component
    pub consensus_controller: Box<dyn ConsensusController>,
    /// link(channels) to the consensus component
    pub consensus_channels: ConsensusChannels,
    /// link to the execution component
    pub execution_controller: Box<dyn ExecutionController>,
    /// link(channels) to the pool component
    pub pool_channels: PoolChannels,
    /// API settings
    pub api_settings: APIConfig,
    /// node version
    pub version: Version,
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

/// Used to manage the API
#[async_trait::async_trait]
pub trait ApiServer: MassaApiServer {
    /// Start the API
    async fn serve(
        self,
        url: &SocketAddr,
        api_config: &APIConfig,
    ) -> Result<StopHandle, JsonRpseeError>;
}

async fn serve<T>(
    api: RpcModule<T>,
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
        .set_batch_request_config(if api_config.batch_request_limit > 0 {
            BatchRequestConfig::Limit(api_config.batch_request_limit)
        } else {
            BatchRequestConfig::Disabled
        })
        .ping_interval(api_config.ping_interval.to_duration())
        .custom_tokio_runtime(tokio::runtime::Handle::current());

    if api_config.enable_http && !api_config.enable_ws {
        server_builder = server_builder.http_only();
    } else if api_config.enable_ws && !api_config.enable_http {
        server_builder = server_builder.ws_only()
    } else if !api_config.enable_http && !api_config.enable_ws {
        panic!("wrong server configuration, you can't disable both http and ws");
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

    let server_handler = server.start(api).expect("server start failed");
    let stop_handler = StopHandle { server_handler };

    Ok(stop_handler)
}

/// Used to be able to stop the API
pub struct StopHandle {
    server_handler: ServerHandle,
}

impl StopHandle {
    /// stop the API gracefully
    pub async fn stop(self) {
        match self.server_handler.stop() {
            Ok(_) => {
                info!("API stop signal sent successfully");
            }
            Err(err) => warn!("API thread panicked: {:?}", err),
        }
        self.server_handler.stopped().await;
    }
}

/// Exposed API methods
#[rpc(server)]
pub trait MassaRpc {
    /// Gracefully stop the node.
    #[method(name = "stop_node")]
    fn stop_node(&self) -> RpcResult<()>;

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

    /// Returns node peers whitelist IP address(es).
    #[method(name = "node_peers_whitelist")]
    async fn node_peers_whitelist(&self) -> RpcResult<Vec<IpAddr>>;

    /// Add IP address(es) to node peers whitelist.
    /// No confirmation to expect.
    /// Note: If the ip was unknown it adds it to the known peers, otherwise it updates the peer type
    #[method(name = "node_add_to_peers_whitelist")]
    async fn node_add_to_peers_whitelist(&self, arg: Vec<IpAddr>) -> RpcResult<()>;

    /// Remove from peers whitelist given IP address(es).
    /// keep it as standard
    /// No confirmation to expect.
    #[method(name = "node_remove_from_peers_whitelist")]
    async fn node_remove_from_peers_whitelist(&self, arg: Vec<IpAddr>) -> RpcResult<()>;

    /// Returns node bootstrap whitelist IP address(es).
    #[method(name = "node_bootstrap_whitelist")]
    async fn node_bootstrap_whitelist(&self) -> RpcResult<Vec<IpAddr>>;

    /// Allow everyone to bootstrap from the node.
    /// remove bootstrap whitelist configuration file.
    #[method(name = "node_bootstrap_whitelist_allow_all")]
    async fn node_bootstrap_whitelist_allow_all(&self) -> RpcResult<()>;

    /// Add IP address(es) to node bootstrap whitelist.
    #[method(name = "node_add_to_bootstrap_whitelist")]
    async fn node_add_to_bootstrap_whitelist(&self, arg: Vec<IpAddr>) -> RpcResult<()>;

    /// Remove IP address(es) to bootstrap whitelist.
    #[method(name = "node_remove_from_bootstrap_whitelist")]
    async fn node_remove_from_bootstrap_whitelist(&self, arg: Vec<IpAddr>) -> RpcResult<()>;

    /// Returns node bootstrap blacklist IP address(es).
    #[method(name = "node_bootstrap_blacklist")]
    async fn node_bootstrap_blacklist(&self) -> RpcResult<Vec<IpAddr>>;

    /// Add IP address(es) to node bootstrap blacklist.
    #[method(name = "node_add_to_bootstrap_blacklist")]
    async fn node_add_to_bootstrap_blacklist(&self, arg: Vec<IpAddr>) -> RpcResult<()>;

    /// Remove IP address(es) to bootstrap blacklist.
    #[method(name = "node_remove_from_bootstrap_blacklist")]
    async fn node_remove_from_bootstrap_blacklist(&self, arg: Vec<IpAddr>) -> RpcResult<()>;

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
    async fn get_stakers(
        &self,
        page_request: Option<PageRequest>,
    ) -> RpcResult<PagedVec<(Address, u64)>>;

    /// Returns operation(s) information associated to a given list of operation(s) ID(s).
    #[method(name = "get_operations")]
    async fn get_operations(&self, arg: Vec<OperationId>) -> RpcResult<Vec<OperationInfo>>;

    /// Returns endorsement(s) information associated to a given list of endorsement(s) ID(s)
    #[method(name = "get_endorsements")]
    async fn get_endorsements(&self, arg: Vec<EndorsementId>) -> RpcResult<Vec<EndorsementInfo>>;

    /// Returns block(s) information associated to a given list of block(s) ID(s)
    #[method(name = "get_blocks")]
    async fn get_blocks(&self, arg: Vec<BlockId>) -> RpcResult<Vec<BlockInfo>>;

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
