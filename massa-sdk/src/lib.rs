// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Utilities for a massa client

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

use http::header::HeaderName;
use jsonrpsee::core::client::{ClientT, IdKind, Subscription, SubscriptionClientT};
use jsonrpsee::http_client::transport::HttpBackend;
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::rpc_params;
use jsonrpsee::types::ErrorObject;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue, WsClient, WsClientBuilder};
use jsonrpsee::{core::RpcResult, http_client::HttpClientBuilder};
use jsonrpsee_http_client as _;
use jsonrpsee_ws_client as _;
use massa_api_exports::page::PagedVecV2;
use massa_api_exports::ApiRequest;
use massa_api_exports::{
    address::AddressInfo,
    block::{BlockInfo, BlockSummary},
    datastore::{DatastoreEntryInput, DatastoreEntryOutput},
    endorsement::EndorsementInfo,
    execution::{ExecuteReadOnlyResponse, ReadOnlyBytecodeExecution, ReadOnlyCall},
    node::NodeStatus,
    operation::{OperationInfo, OperationInput},
    TimeInterval,
};
use massa_models::secure_share::SecureShare;
use massa_models::{
    address::Address,
    block::FilledBlock,
    block_header::BlockHeader,
    block_id::BlockId,
    clique::Clique,
    composite::PubkeySig,
    endorsement::EndorsementId,
    execution::EventFilter,
    node::NodeId,
    operation::{Operation, OperationId},
    output_event::SCOutputEvent,
    prehash::{PreHashMap, PreHashSet},
    version::Version,
};
use massa_proto::massa::api::v1::massa_service_client::MassaServiceClient;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use thiserror::Error;

mod config;
pub use config::ClientConfig;
pub use config::HttpConfig;
pub use config::WsConfig;

/// Error when creating a new client
#[derive(Error, Debug)]
pub enum ClientError {
    /// Url error
    #[error("Invalid grpc url: {0}")]
    Url(#[from] http::uri::InvalidUri),
    /// Connection error
    #[error("Cannot connect to grpc server: {0}")]
    Connect(#[from] tonic::transport::Error),
}

/// Client
pub struct Client {
    /// public component
    pub public: RpcClient,
    /// private component
    pub private: RpcClient,
    /// grpc client
    pub grpc: Option<MassaServiceClient<tonic::transport::Channel>>,
}

impl Client {
    /// creates a new client
    pub async fn new(
        ip: IpAddr,
        public_port: u16,
        private_port: u16,
        grpc_port: u16,
        http_config: &HttpConfig,
    ) -> Result<Client, ClientError> {
        let public_socket_addr = SocketAddr::new(ip, public_port);
        let private_socket_addr = SocketAddr::new(ip, private_port);
        let grpc_socket_addr = SocketAddr::new(ip, grpc_port);
        let public_url = format!("http://{}", public_socket_addr);
        let private_url = format!("http://{}", private_socket_addr);
        let grpc_url = format!("grpc://{}", grpc_socket_addr);

        // try to start grpc client and connect to the server
        let grpc_opts = match tonic::transport::Channel::from_shared(grpc_url)?
            .connect()
            .await
        {
            Ok(channel) => Some(MassaServiceClient::new(channel)),
            Err(e) => {
                tracing::warn!("unable to connect to grpc server {}", e);
                None
            }
        };

        Ok(Client {
            public: RpcClient::from_url(&public_url, http_config).await,
            private: RpcClient::from_url(&private_url, http_config).await,
            grpc: grpc_opts,
        })
    }
}

/// Rpc client
pub struct RpcClient {
    http_client: HttpClient<HttpBackend>,
}

impl RpcClient {
    /// Default constructor
    pub async fn from_url(url: &str, http_config: &HttpConfig) -> RpcClient {
        RpcClient {
            http_client: http_client_from_url(url, http_config),
        }
    }

    /// Gracefully stop the node.
    pub async fn stop_node(&self) -> RpcResult<()> {
        self.http_client
            .request("stop_node", rpc_params![])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Sign message with node's key.
    /// Returns the public key that signed the message and the signature.
    pub async fn node_sign_message(&self, message: Vec<u8>) -> RpcResult<PubkeySig> {
        self.http_client
            .request("node_sign_message", rpc_params![message])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Add a vector of new secret keys for the node to use to stake.
    /// No confirmation to expect.
    pub async fn add_staking_secret_keys(&self, secret_keys: Vec<String>) -> RpcResult<()> {
        self.http_client
            .request("add_staking_secret_keys", rpc_params![secret_keys])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Remove a vector of addresses used to stake.
    /// No confirmation to expect.
    pub async fn remove_staking_addresses(&self, addresses: Vec<Address>) -> RpcResult<()> {
        self.http_client
            .request("remove_staking_addresses", rpc_params![addresses])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Return hash-set of staking addresses.
    pub async fn get_staking_addresses(&self) -> RpcResult<PreHashSet<Address>> {
        self.http_client
            .request("get_staking_addresses", rpc_params![])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Bans given ip address(es)
    /// No confirmation to expect.
    pub async fn node_ban_by_ip(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_ban_by_ip", rpc_params![ips])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Bans given node id(s)
    /// No confirmation to expect.
    pub async fn node_ban_by_id(&self, ids: Vec<NodeId>) -> RpcResult<()> {
        self.http_client
            .request("node_ban_by_id", rpc_params![ids])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Unban given ip address(es)
    /// No confirmation to expect.
    pub async fn node_unban_by_ip(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_unban_by_ip", rpc_params![ips])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Unban given node id(s)
    /// No confirmation to expect.
    pub async fn node_unban_by_id(&self, ids: Vec<NodeId>) -> RpcResult<()> {
        self.http_client
            .request("node_unban_by_id", rpc_params![ids])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Returns node peers whitelist IP address(es).
    pub async fn node_peers_whitelist(&self) -> RpcResult<Vec<IpAddr>> {
        self.http_client
            .request("node_peers_whitelist", rpc_params![])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Add IP address(es) to node peers whitelist.
    pub async fn node_add_to_peers_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_add_to_peers_whitelist", rpc_params![ips])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Remove IP address(es) to node peers whitelist.
    pub async fn node_remove_from_peers_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_remove_from_peers_whitelist", rpc_params![ips])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Returns node bootstrap whitelist IP address(es).
    pub async fn node_bootstrap_whitelist(&self) -> RpcResult<Vec<IpAddr>> {
        self.http_client
            .request("node_bootstrap_whitelist", rpc_params![])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Allow everyone to bootstrap from the node.
    /// remove bootstrap whitelist configuration file.
    pub async fn node_bootstrap_whitelist_allow_all(&self) -> RpcResult<()> {
        self.http_client
            .request("node_bootstrap_whitelist_allow_all", rpc_params![])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Add IP address(es) to node bootstrap whitelist.
    pub async fn node_add_to_bootstrap_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_add_to_bootstrap_whitelist", rpc_params![ips])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Remove IP address(es) to bootstrap whitelist.
    pub async fn node_remove_from_bootstrap_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_remove_from_bootstrap_whitelist", rpc_params![ips])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Returns node bootstrap blacklist IP address(es).
    pub async fn node_bootstrap_blacklist(&self) -> RpcResult<Vec<IpAddr>> {
        self.http_client
            .request("node_bootstrap_blacklist", rpc_params![])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Add IP address(es) to node bootstrap blacklist.
    pub async fn node_add_to_bootstrap_blacklist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_add_to_bootstrap_blacklist", rpc_params![ips])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Remove IP address(es) to bootstrap blacklist.
    pub async fn node_remove_from_bootstrap_blacklist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_remove_from_bootstrap_blacklist", rpc_params![ips])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    ////////////////
    // public-api //
    ////////////////

    // Explorer (aggregated stats)

    /// summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count
    pub async fn get_status(&self) -> RpcResult<NodeStatus> {
        self.http_client
            .request("get_status", rpc_params![])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    pub(crate) async fn _get_cliques(&self) -> RpcResult<Vec<Clique>> {
        self.http_client
            .request("get_cliques", rpc_params![])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    // Debug (specific information)

    /// Returns the active stakers and their roll counts for the current cycle.
    pub(crate) async fn _get_stakers(&self) -> RpcResult<PreHashMap<Address, u64>> {
        self.http_client
            .request("get_stakers", rpc_params![])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Returns operation(s) information associated to a given list of operation(s) ID(s).
    pub async fn get_operations(
        &self,
        operation_ids: Vec<OperationId>,
    ) -> RpcResult<Vec<OperationInfo>> {
        self.http_client
            .request("get_operations", rpc_params![operation_ids])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Returns endorsement(s) information associated to a given list of endorsement(s) ID(s)
    pub async fn get_endorsements(
        &self,
        endorsement_ids: Vec<EndorsementId>,
    ) -> RpcResult<Vec<EndorsementInfo>> {
        self.http_client
            .request("get_endorsements", rpc_params![endorsement_ids])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Returns block(s) information associated to a given list of block(s) ID(s)
    pub async fn get_blocks(&self, block_ids: Vec<BlockId>) -> RpcResult<Vec<BlockInfo>> {
        self.http_client
            .request("get_blocks", rpc_params![block_ids])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Get events emitted by smart contracts with various filters
    pub async fn get_filtered_sc_output_event(
        &self,
        filter: EventFilter,
    ) -> RpcResult<Vec<SCOutputEvent>> {
        self.http_client
            .request("get_filtered_sc_output_event", rpc_params![filter])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Get the block graph within the specified time interval.
    /// Optional parameters: from `<time_start>` (included) and to `<time_end>` (excluded) millisecond timestamp
    pub(crate) async fn _get_graph_interval(
        &self,
        time_interval: TimeInterval,
    ) -> RpcResult<Vec<BlockSummary>> {
        self.http_client
            .request("get_graph_interval", rpc_params![time_interval])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Get info by addresses
    pub async fn get_addresses(&self, addresses: Vec<Address>) -> RpcResult<Vec<AddressInfo>> {
        self.http_client
            .request("get_addresses", rpc_params![addresses])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// Get datastore entries
    pub async fn get_datastore_entries(
        &self,
        input: Vec<DatastoreEntryInput>,
    ) -> RpcResult<Vec<DatastoreEntryOutput>> {
        self.http_client
            .request("get_datastore_entries", rpc_params![input])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    // User (interaction with the node)

    /// Adds operations to pool. Returns operations that were ok and sent to pool.
    pub async fn send_operations(
        &self,
        operations: Vec<OperationInput>,
    ) -> RpcResult<Vec<OperationId>> {
        self.http_client
            .request("send_operations", rpc_params![operations])
            .await
            .map_err(|e| to_error_obj(e.to_string()))
    }

    /// execute read only bytecode
    pub async fn execute_read_only_bytecode(
        &self,
        read_only_execution: ReadOnlyBytecodeExecution,
    ) -> RpcResult<ExecuteReadOnlyResponse> {
        self.http_client
            .request::<Vec<ExecuteReadOnlyResponse>, Vec<Vec<ReadOnlyBytecodeExecution>>>(
                "execute_read_only_bytecode",
                vec![vec![read_only_execution]],
            )
            .await
            .map_err(|e| to_error_obj(e.to_string()))?
            .pop()
            .ok_or_else(|| {
                to_error_obj("missing return value on execute_read_only_bytecode".to_owned())
            })
    }

    /// execute read only SC call
    pub async fn execute_read_only_call(
        &self,
        read_only_execution: ReadOnlyCall,
    ) -> RpcResult<ExecuteReadOnlyResponse> {
        self.http_client
            .request::<Vec<ExecuteReadOnlyResponse>, Vec<Vec<ReadOnlyCall>>>(
                "execute_read_only_call",
                vec![vec![read_only_execution]],
            )
            .await
            .map_err(|e| to_error_obj(e.to_string()))?
            .pop()
            .ok_or_else(|| {
                to_error_obj("missing return value on execute_read_only_call".to_owned())
            })
    }
}

/// Client V2
pub struct ClientV2 {
    /// API V2 component
    pub api: RpcClientV2,
}

impl ClientV2 {
    /// creates a new client
    pub async fn new(
        ip: IpAddr,
        api_port: u16,
        http_config: &HttpConfig,
        ws_config: &WsConfig,
    ) -> ClientV2 {
        let api_socket_addr = SocketAddr::new(ip, api_port);
        ClientV2 {
            api: RpcClientV2::from_url(api_socket_addr, http_config, ws_config).await,
        }
    }
}

/// Rpc V2 client
pub struct RpcClientV2 {
    http_client: Option<HttpClient<HttpBackend>>,
    ws_client: Option<WsClient>,
}

impl RpcClientV2 {
    /// Default constructor
    pub async fn from_url(
        socket_addr: SocketAddr,
        http_config: &HttpConfig,
        ws_config: &WsConfig,
    ) -> RpcClientV2 {
        let http_url = format!("http://{}", socket_addr);
        let ws_url = format!("ws://{}", socket_addr);

        if http_config.enabled && !ws_config.enabled {
            let http_client = http_client_from_url(&http_url, http_config);
            return RpcClientV2 {
                http_client: Some(http_client),
                ws_client: None,
            };
        } else if !http_config.enabled && ws_config.enabled {
            let ws_client = ws_client_from_url(&ws_url, ws_config).await;
            return RpcClientV2 {
                http_client: None,
                ws_client: Some(ws_client),
            };
        } else if !http_config.enabled && !ws_config.enabled {
            panic!("wrong client configuration, you can't disable both http and ws");
        }

        let http_client = http_client_from_url(&http_url, http_config);
        let ws_client = ws_client_from_url(&ws_url, ws_config).await;

        RpcClientV2 {
            http_client: Some(http_client),
            ws_client: Some(ws_client),
        }
    }

    ////////////////
    //   API V2   //
    ////////////////
    //
    // Experimental APIs. They might disappear, and they will change //

    /// Get the active stakers and their active roll counts for the current cycle sorted by largest roll counts.
    pub async fn get_largest_stakers(
        &self,
        request: Option<ApiRequest>,
    ) -> RpcResult<PagedVecV2<(BlockId, u64)>> {
        if let Some(client) = self.http_client.as_ref() {
            client
                .request("get_largest_stakers", rpc_params![request])
                .await
                .map_err(|e| to_error_obj(e.to_string()))
        } else {
            Err(to_error_obj("no Http client instance found".to_owned()))
        }
    }

    /// Get the ids of best parents for the next block to be produced along with their period
    pub async fn get_next_block_best_parents(&self) -> RpcResult<Vec<(BlockId, u64)>> {
        if let Some(client) = self.http_client.as_ref() {
            client
                .request("get_next_block_best_parents", rpc_params![])
                .await
                .map_err(|e| to_error_obj(e.to_string()))
        } else {
            Err(to_error_obj("no Http client instance found".to_owned()))
        }
    }

    /// Get Massa node version
    pub async fn get_version(&self) -> RpcResult<Version> {
        if let Some(client) = self.http_client.as_ref() {
            client.request("get_version", rpc_params![]).await.unwrap()
        } else {
            Err(to_error_obj("no Http client instance found".to_owned()))
        }
    }

    /// New produced blocks
    pub async fn subscribe_new_blocks(
        &self,
    ) -> Result<Subscription<BlockInfo>, jsonrpsee::core::Error> {
        if let Some(client) = self.ws_client.as_ref() {
            client
                .subscribe(
                    "subscribe_new_blocks",
                    rpc_params![],
                    "unsubscribe_new_blocks",
                )
                .await
        } else {
            Err(to_error_obj("no WebSocket client instance found".to_owned()).into())
        }
    }

    /// New produced blocks headers
    pub async fn subscribe_new_blocks_headers(
        &self,
    ) -> Result<Subscription<SecureShare<BlockHeader, BlockId>>, jsonrpsee::core::Error> {
        if let Some(client) = self.ws_client.as_ref() {
            client
                .subscribe(
                    "subscribe_new_blocks_headers",
                    rpc_params![],
                    "unsubscribe_new_blocks_headers",
                )
                .await
        } else {
            Err(to_error_obj("no WebSocket client instance found".to_owned()).into())
        }
    }

    /// New produced blocks with operations content.
    pub async fn subscribe_new_filled_blocks(
        &self,
    ) -> Result<Subscription<FilledBlock>, jsonrpsee::core::Error> {
        if let Some(client) = self.ws_client.as_ref() {
            client
                .subscribe(
                    "subscribe_new_filled_blocks",
                    rpc_params![],
                    "unsubscribe_new_filled_blocks",
                )
                .await
        } else {
            Err(to_error_obj("no WebSocket client instance found".to_owned()).into())
        }
    }

    /// New produced operations.
    pub async fn subscribe_new_operations(
        &self,
    ) -> Result<Subscription<Operation>, jsonrpsee::core::Error> {
        if let Some(client) = self.ws_client.as_ref() {
            client
                .subscribe(
                    "subscribe_new_operations",
                    rpc_params![],
                    "unsubscribe_new_operations",
                )
                .await
        } else {
            Err(to_error_obj("no WebSocket client instance found".to_owned()).into())
        }
    }
}

fn http_client_from_url(url: &str, http_config: &HttpConfig) -> HttpClient<HttpBackend> {
    let mut builder = HttpClientBuilder::default()
        .max_request_size(http_config.client_config.max_request_body_size)
        .request_timeout(http_config.client_config.request_timeout.to_duration())
        .max_concurrent_requests(http_config.client_config.max_concurrent_requests)
        .id_format(get_id_kind(http_config.client_config.id_kind.as_str()))
        .set_headers(get_headers(&http_config.client_config.headers));

    match http_config.client_config.certificate_store.as_str() {
        "Native" => builder = builder.use_native_rustls(),
        "WebPki" => builder = builder.use_webpki_rustls(),
        _ => {}
    }

    builder
        .build(url)
        .unwrap_or_else(|_| panic!("unable to create Http client for {}", url))
}

async fn ws_client_from_url(url: &str, ws_config: &WsConfig) -> WsClient
where
    WsClient: SubscriptionClientT,
{
    let mut builder = WsClientBuilder::default()
        .max_request_size(ws_config.client_config.max_request_body_size)
        .request_timeout(ws_config.client_config.request_timeout.to_duration())
        .max_concurrent_requests(ws_config.client_config.max_concurrent_requests)
        .id_format(get_id_kind(ws_config.client_config.id_kind.as_str()))
        .set_headers(get_headers(&ws_config.client_config.headers))
        .max_buffer_capacity_per_subscription(ws_config.max_notifs_per_subscription)
        .max_redirections(ws_config.max_redirections);

    match ws_config.client_config.certificate_store.as_str() {
        "Native" => builder = builder.use_native_rustls(),
        "WebPki" => builder = builder.use_webpki_rustls(),
        _ => {}
    }

    builder
        .build(url)
        .await
        .unwrap_or_else(|_| panic!("unable to create WebSocket client for {}", url))
}

fn get_id_kind(id_kind: &str) -> IdKind {
    match id_kind {
        "Number" => IdKind::Number,
        "String" => IdKind::String,
        _ => IdKind::Number,
    }
}

fn get_headers(headers: &[(String, String)]) -> HeaderMap {
    let mut headers_map = HeaderMap::new();
    headers.iter().for_each(|(key, value)| {
        let header_name = match HeaderName::from_str(key.as_str()) {
            Ok(header_name) => header_name,
            Err(_) => panic!("invalid header name: {:?}", key),
        };
        let header_value = match HeaderValue::from_str(value.as_str()) {
            Ok(header_name) => header_name,
            Err(_) => panic!("invalid header value: {:?}", value),
        };
        headers_map.insert(header_name, header_value);
    });

    headers_map
}

// SDK error object
fn to_error_obj(message: String) -> ErrorObject<'static> {
    ErrorObject::owned(-32080, message, None::<()>)
}
