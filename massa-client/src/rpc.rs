// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::SETTINGS;
use jsonrpc_core_client::transports::http;
use jsonrpc_core_client::{RpcChannel, RpcError, RpcResult, TypedClient};
use massa_models::api::{
    AddressInfo, BlockInfo, BlockSummary, EndorsementInfo, NodeStatus, OperationInfo,
    ReadOnlyExecution, TimeInterval,
};
use massa_models::clique::Clique;
use massa_models::composite::PubkeySig;
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::prehash::{Map, Set};
use massa_models::{Address, BlockId, EndorsementId, OperationId, SignedOperation};
use massa_signature::PrivateKey;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
pub struct Client {
    pub public: RpcClient,
    pub private: RpcClient,
}

impl Client {
    pub(crate) async fn new(ip: IpAddr, public_port: u16, private_port: u16) -> Client {
        let public_socket_addr = SocketAddr::new(ip, public_port);
        let private_socket_addr = SocketAddr::new(ip, private_port);
        let public_url = format!("http://{}", public_socket_addr);
        let private_url = format!("http://{}", private_socket_addr);
        Client {
            public: RpcClient::from_url(&public_url).await,
            private: RpcClient::from_url(&private_url).await,
        }
    }
}

pub struct RpcClient(TypedClient);

/// This is required by `jsonrpc_core_client::transports::http::connect`
impl From<RpcChannel> for RpcClient {
    fn from(channel: RpcChannel) -> Self {
        RpcClient(channel.into())
    }
}

impl RpcClient {
    /// Default constructor
    pub(crate) async fn from_url(url: &str) -> RpcClient {
        match http::connect::<RpcClient>(url).await {
            Ok(client) => client,
            Err(_) => panic!("unable to connect to Node."),
        }
    }

    /// Typed wrapper to API calls based on the method given by `jsonrpc_core_client`
    async fn call_method<T: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        returns: &str,
        args: T,
    ) -> RpcResult<R> {
        let time = SETTINGS.timeout.to_duration();
        tokio::time::timeout(time, self.0.call_method(method, returns, args))
            .await
            .map_err(|e| RpcError::Client(format!("timeout during {}: {}", method, e)))?
    }

    /// Gracefully stop the node.
    pub(crate) async fn stop_node(&self) -> RpcResult<()> {
        self.call_method("stop_node", "()", ()).await
    }

    /// Sign message with node's key.
    /// Returns the public key that signed the message and the signature.
    pub(crate) async fn node_sign_message(&self, message: Vec<u8>) -> RpcResult<PubkeySig> {
        self.call_method("node_sign_message", "PubkeySig", vec![message])
            .await
    }

    /// Add a vec of new private keys for the node to use to stake.
    /// No confirmation to expect.
    pub(crate) async fn add_staking_private_keys(
        &self,
        private_keys: Vec<PrivateKey>,
    ) -> RpcResult<()> {
        self.call_method("add_staking_private_keys", "()", vec![private_keys])
            .await
    }

    /// Remove a vec of addresses used to stake.
    /// No confirmation to expect.
    pub(crate) async fn remove_staking_addresses(&self, addresses: Vec<Address>) -> RpcResult<()> {
        self.call_method("remove_staking_addresses", "()", vec![addresses])
            .await
    }

    /// Return hashset of staking addresses.
    pub(crate) async fn get_staking_addresses(&self) -> RpcResult<Set<Address>> {
        self.call_method("get_staking_addresses", "Set<Address>", ())
            .await
    }

    /// Bans given node id
    /// No confirmation to expect.
    pub(crate) async fn ban(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.call_method("ban", "()", vec![ips]).await
    }

    /// Unbans given ip addr
    /// No confirmation to expect.
    pub(crate) async fn unban(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.call_method("unban", "()", vec![ips]).await
    }

    /// execute read only bytecode
    pub(crate) async fn execute_read_only_request(
        &self,
        read_only_execution: ReadOnlyExecution,
    ) -> RpcResult<ExecuteReadOnlyResponse> {
        self.call_method::<Vec<Vec<ReadOnlyExecution>>, Vec<ExecuteReadOnlyResponse>>(
            "execute_read_only_request",
            "Vec<ExecuteReadOnlyResponse>",
            vec![vec![read_only_execution]],
        )
        .await?
        .pop()
        .ok_or_else(|| RpcError::Client("missing return value on execute_read_only_request".into()))
    }

    ////////////////
    // public-api //
    ////////////////

    // Explorer (aggregated stats)

    /// summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count
    pub(crate) async fn get_status(&self) -> RpcResult<NodeStatus> {
        self.call_method("get_status", "NodeStatus", ()).await
    }

    pub(crate) async fn _get_cliques(&self) -> RpcResult<Vec<Clique>> {
        self.call_method("get_cliques", "Vec<Clique>", ()).await
    }

    // Debug (specific information)

    /// Returns the active stakers and their roll counts for the current cycle.
    pub(crate) async fn _get_stakers(&self) -> RpcResult<Map<Address, u64>> {
        self.call_method("get_stakers", "Map<Address, u64>", ())
            .await
    }

    /// Returns operations information associated to a given list of operations' IDs.
    pub(crate) async fn get_operations(
        &self,
        operation_ids: Vec<OperationId>,
    ) -> RpcResult<Vec<OperationInfo>> {
        self.call_method("get_operations", "Vec<OperationInfo>", vec![operation_ids])
            .await
    }

    pub(crate) async fn get_endorsements(
        &self,
        endorsement_ids: Vec<EndorsementId>,
    ) -> RpcResult<Vec<EndorsementInfo>> {
        self.call_method(
            "get_endorsements",
            "Vec<EndorsementInfo>",
            vec![endorsement_ids],
        )
        .await
    }

    /// Get information on a block given its BlockId
    pub(crate) async fn get_block(&self, block_id: BlockId) -> RpcResult<BlockInfo> {
        self.call_method("get_block", "BlockInfo", vec![block_id])
            .await
    }

    /// Get the block graph within the specified time interval.
    /// Optional parameters: from <time_start> (included) and to <time_end> (excluded) millisecond timestamp
    pub(crate) async fn _get_graph_interval(
        &self,
        time_interval: TimeInterval,
    ) -> RpcResult<Vec<BlockSummary>> {
        self.call_method("get_graph_interval", "Vec<BlockSummary>", time_interval)
            .await
    }

    pub(crate) async fn get_addresses(
        &self,
        addresses: Vec<Address>,
    ) -> RpcResult<Vec<AddressInfo>> {
        self.call_method("get_addresses", "Vec<AddressInfo>", vec![addresses])
            .await
    }

    // User (interaction with the node)

    /// Adds operations to pool. Returns operations that were ok and sent to pool.
    pub(crate) async fn send_operations(
        &self,
        operations: Vec<SignedOperation>,
    ) -> RpcResult<Vec<OperationId>> {
        self.call_method("send_operations", "Vec<OperationId>", vec![operations])
            .await
    }
}
