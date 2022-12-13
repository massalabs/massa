// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Utilities for a massa client

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

use http::header::HeaderName;
use jsonrpsee::core::client::{CertificateStore, ClientT, IdKind};
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::rpc_params;
use jsonrpsee::ws_client::{HeaderMap, HeaderValue};
use massa_models::api::{
    AddressInfo, BlockInfo, BlockSummary, DatastoreEntryInput, DatastoreEntryOutput,
    EndorsementInfo, EventFilter, NodeStatus, OperationInfo, OperationInput,
    ReadOnlyBytecodeExecution, ReadOnlyCall, TimeInterval,
};
use massa_models::clique::Clique;
use massa_models::composite::PubkeySig;
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::node::NodeId;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::{PreHashMap, PreHashSet};
use massa_models::{
    address::Address, block::BlockId, endorsement::EndorsementId, operation::OperationId,
};

use jsonrpsee::{core::Error as JsonRpseeError, core::RpcResult, http_client::HttpClientBuilder};
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

mod config;
pub use config::HttpConfig;

/// Client
pub struct Client {
    /// public component
    pub public: RpcClient,
    /// private component
    pub private: RpcClient,
}

impl Client {
    /// creates a new client
    pub async fn new(
        ip: IpAddr,
        public_port: u16,
        private_port: u16,
        http_config: &HttpConfig,
    ) -> Client {
        let public_socket_addr = SocketAddr::new(ip, public_port);
        let private_socket_addr = SocketAddr::new(ip, private_port);
        let public_url = format!("http://{}", public_socket_addr);
        let private_url = format!("http://{}", private_socket_addr);
        Client {
            public: RpcClient::from_url(&public_url, http_config).await,
            private: RpcClient::from_url(&private_url, http_config).await,
        }
    }
}

/// TODO add ws client
pub struct RpcClient {
    http_client: HttpClient,
}

impl RpcClient {
    /// Default constructor
    pub async fn from_url(url: &str, http_config: &HttpConfig) -> RpcClient {
        let certificate_store = match http_config.certificate_store.as_str() {
            "Native" => CertificateStore::Native,
            "WebPki" => CertificateStore::WebPki,
            _ => CertificateStore::Native,
        };
        let id_kind = match http_config.id_kind.as_str() {
            "Number" => IdKind::Number,
            "String" => IdKind::String,
            _ => IdKind::Number,
        };

        let mut headers = HeaderMap::new();
        http_config.headers.iter().for_each(|(key, value)| {
            let header_name = match HeaderName::from_str(key.as_str()) {
                Ok(header_name) => header_name,
                Err(_) => panic!("invalid header name: {:?}", key),
            };
            let header_value = match HeaderValue::from_str(value.as_str()) {
                Ok(header_name) => header_name,
                Err(_) => panic!("invalid header value: {:?}", value),
            };
            headers.insert(header_name, header_value);
        });

        match HttpClientBuilder::default()
            .max_request_body_size(http_config.max_request_body_size)
            .request_timeout(http_config.request_timeout.to_duration())
            .max_concurrent_requests(http_config.max_concurrent_requests)
            .certificate_store(certificate_store)
            .id_format(id_kind)
            .set_headers(headers)
            .build(url)
        {
            Ok(http_client) => RpcClient { http_client },
            Err(_) => panic!("unable to connect to Node."),
        }
    }

    /// Gracefully stop the node.
    pub async fn stop_node(&self) -> RpcResult<()> {
        self.http_client.request("stop_node", rpc_params![]).await
    }

    /// Sign message with node's key.
    /// Returns the public key that signed the message and the signature.
    pub async fn node_sign_message(&self, message: Vec<u8>) -> RpcResult<PubkeySig> {
        self.http_client
            .request("node_sign_message", rpc_params![message])
            .await
    }

    /// Add a vector of new secret keys for the node to use to stake.
    /// No confirmation to expect.
    pub async fn add_staking_secret_keys(&self, secret_keys: Vec<String>) -> RpcResult<()> {
        self.http_client
            .request("add_staking_secret_keys", rpc_params![secret_keys])
            .await
    }

    /// Remove a vector of addresses used to stake.
    /// No confirmation to expect.
    pub async fn remove_staking_addresses(&self, addresses: Vec<Address>) -> RpcResult<()> {
        self.http_client
            .request("remove_staking_addresses", rpc_params![addresses])
            .await
    }

    /// Return hash-set of staking addresses.
    pub async fn get_staking_addresses(&self) -> RpcResult<PreHashSet<Address>> {
        self.http_client
            .request("get_staking_addresses", rpc_params![])
            .await
    }

    /// Bans given ip address(es)
    /// No confirmation to expect.
    pub async fn node_ban_by_ip(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_ban_by_ip", rpc_params![ips])
            .await
    }

    /// Bans given node id(s)
    /// No confirmation to expect.
    pub async fn node_ban_by_id(&self, ids: Vec<NodeId>) -> RpcResult<()> {
        self.http_client
            .request("node_ban_by_id", rpc_params![ids])
            .await
    }

    /// Unban given ip address(es)
    /// No confirmation to expect.
    pub async fn node_unban_by_ip(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_unban_by_ip", rpc_params![ips])
            .await
    }

    /// Unban given node id(s)
    /// No confirmation to expect.
    pub async fn node_unban_by_id(&self, ids: Vec<NodeId>) -> RpcResult<()> {
        self.http_client
            .request("node_unban_by_id", rpc_params![ids])
            .await
    }

    /// add ips to whitelist
    /// create peer if it was unknown
    pub async fn node_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_whitelist", rpc_params![ips])
            .await
    }

    /// remove IPs from whitelist
    pub async fn node_remove_from_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
            .request("node_remove_from_whitelist", rpc_params![ips])
            .await
    }

    /// Returns node bootsrap whitelist IP address(es).
    pub async fn node_bootstrap_whitelist(&self) -> RpcResult<Vec<IpAddr>> {
        self.http_client
        .request("node_bootstrap_whitelist", rpc_params![])
        .await
    }
    
    /// Add IP address(es) to node bootsrap whitelist.
    pub async fn node_add_to_bootstrap_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
        .request("node_add_to_bootstrap_whitelist", rpc_params![ips])
        .await
    }
    
    /// Remove IP address(es) to bootsrap whitelist.
    pub async fn node_remove_from_bootstrap_whitelist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
        .request("node_remove_from_bootstrap_whitelist", rpc_params![ips])
        .await
    }
    
    /// Returns node bootsrap blacklist IP address(es).
    pub async fn node_bootstrap_blacklist(&self) -> RpcResult<Vec<IpAddr>> {
        self.http_client
        .request("node_bootstrap_blacklist", rpc_params![])
        .await
    }
    
    /// Add IP address(es) to node bootsrap blacklist.
    pub async fn node_add_to_bootstrap_blacklist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
        .request("node_add_to_bootstrap_blacklist", rpc_params![ips])
        .await
    }
    
    /// Remove IP address(es) to bootsrap blacklist.
    pub async fn node_remove_from_bootstrap_blacklist(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.http_client
        .request("node_remove_from_bootstrap_blacklist", rpc_params![ips])
        .await
    }

    ////////////////
    // public-api //
    ////////////////

    // Explorer (aggregated stats)

    /// summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count
    pub async fn get_status(&self) -> RpcResult<NodeStatus> {
        self.http_client.request("get_status", rpc_params![]).await
    }

    pub(crate) async fn _get_cliques(&self) -> RpcResult<Vec<Clique>> {
        self.http_client.request("get_cliques", rpc_params![]).await
    }

    // Debug (specific information)

    /// Returns the active stakers and their roll counts for the current cycle.
    pub(crate) async fn _get_stakers(&self) -> RpcResult<PreHashMap<Address, u64>> {
        self.http_client.request("get_stakers", rpc_params![]).await
    }

    /// Returns operations information associated to a given list of operations' IDs.
    pub async fn get_operations(
        &self,
        operation_ids: Vec<OperationId>,
    ) -> RpcResult<Vec<OperationInfo>> {
        self.http_client
            .request("get_operations", rpc_params![operation_ids])
            .await
    }

    /// get info on endorsements by ids
    pub async fn get_endorsements(
        &self,
        endorsement_ids: Vec<EndorsementId>,
    ) -> RpcResult<Vec<EndorsementInfo>> {
        self.http_client
            .request("get_endorsements", rpc_params![endorsement_ids])
            .await
    }

    /// Get information on a block given its `BlockId`
    pub async fn get_block(&self, block_id: BlockId) -> RpcResult<BlockInfo> {
        self.http_client
            .request("get_block", rpc_params![block_id])
            .await
    }

    /// Get events emitted by smart contracts with various filters
    pub async fn get_filtered_sc_output_event(
        &self,
        filter: EventFilter,
    ) -> RpcResult<Vec<SCOutputEvent>> {
        self.http_client
            .request("get_filtered_sc_output_event", rpc_params![filter])
            .await
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
    }

    /// Get info by addresses
    pub async fn get_addresses(&self, addresses: Vec<Address>) -> RpcResult<Vec<AddressInfo>> {
        self.http_client
            .request("get_addresses", rpc_params![addresses])
            .await
    }

    /// Get datastore entries
    pub async fn get_datastore_entries(
        &self,
        input: Vec<DatastoreEntryInput>,
    ) -> RpcResult<Vec<DatastoreEntryOutput>> {
        self.http_client
            .request("get_datastore_entries", rpc_params![input])
            .await
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
            .await?
            .pop()
            .ok_or_else(|| {
                JsonRpseeError::Custom("missing return value on execute_read_only_bytecode".into())
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
            .await?
            .pop()
            .ok_or_else(|| {
                JsonRpseeError::Custom("missing return value on execute_read_only_call".into())
            })
    }
}
