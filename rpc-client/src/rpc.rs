// Copyright (c) 2021 MASSA LABS <info@massa.net>

use api_dto::{
    AddressInfo, BlockInfo, BlockSummary, EndorsementInfo, NodeStatus, OperationInfo, RollsInfo,
    TimeInterval,
};
use crypto::signature::PrivateKey;
use jsonrpc_core_client::transports::http;
use jsonrpc_core_client::{RpcChannel, RpcResult, TypedClient};
use models::address::{AddressHashMap, AddressHashSet};
use models::clique::Clique;
use models::crypto::PubkeySig;
use models::node::NodeId;
use models::{Address, AlgoConfig, BlockId, EndorsementId, Operation, OperationId};
use std::net::IpAddr;

// TODO: This crate should at some point be renamed `client`, `massa` or `massa-client`
// and replace the previous one!

pub struct Client {
    pub public: RpcClient,
    pub private: RpcClient,
}

impl Client {
    pub(crate) async fn new(ip: IpAddr, public_port: u16, private_port: u16) -> Client {
        let public_url = format!("http://{}:{}", ip, public_port);
        let private_url = format!("http://{}:{}", ip, private_port);
        Client {
            public: RpcClient::from_url(&public_url).await,
            private: RpcClient::from_url(&private_url).await,
        }
    }
}

// TODO: Did we crate 2 RpcClient structs? (to separate public/private calls in impl)
pub struct RpcClient(TypedClient);

/// This is required by `jsonrpc_core_client::transports::http::connect`
impl From<RpcChannel> for RpcClient {
    fn from(channel: RpcChannel) -> Self {
        RpcClient(channel.into())
    }
}

/// Typed wrapper to API calls based on the method given by `jsonrpc_core_client`:
///
/// ```rust
/// fn call_method<T: Serialize, R: DeserializeOwned>(
///     method: &str,
///     returns: &str,
///     args: T,
/// ) -> impl Future<Output = RpcResult<R>> {
/// }
/// ```
impl RpcClient {
    /// Default constructor
    pub(crate) async fn from_url(url: &str) -> RpcClient {
        match http::connect::<RpcClient>(&url).await {
            Ok(client) => client,
            Err(_) => panic!("Unable to connect to Node."),
        }
    }

    /////////////////
    // private-api //
    /////////////////

    /// Gracefully stop the node.
    pub(crate) async fn stop_node(&self) -> RpcResult<()> {
        self.0.call_method("stop_node", "()", ()).await
    }

    /// Sign message with node's key.
    /// Returns the public key that signed the message and the signature.
    pub(crate) async fn node_sign_message(&self, message: Vec<u8>) -> RpcResult<PubkeySig> {
        self.0
            .call_method("node_sign_message", "PubkeySig", vec![message])
            .await
    }

    /// Add a vec of new private keys for the node to use to stake.
    /// No confirmation to expect.
    pub(crate) async fn add_staking_private_keys(
        &self,
        private_keys: Vec<PrivateKey>,
    ) -> RpcResult<()> {
        self.0
            .call_method("add_staking_private_keys", "()", vec![private_keys])
            .await
    }

    /// Remove a vec of addresses used to stake.
    /// No confirmation to expect.
    pub(crate) async fn remove_staking_addresses(&self, addresses: Vec<Address>) -> RpcResult<()> {
        self.0
            .call_method("remove_staking_addresses", "()", vec![addresses])
            .await
    }

    /// Return hashset of staking addresses.
    pub(crate) async fn get_staking_addresses(&self) -> RpcResult<AddressHashSet> {
        self.0
            .call_method("get_staking_addresses", "AddressHashSet", ())
            .await
    }

    /// Bans given node id
    /// No confirmation to expect.
    pub(crate) async fn ban(&self, node_id: NodeId) -> RpcResult<()> {
        self.0.call_method("ban", "()", vec![node_id]).await
    }

    /// Unbans given ip addr
    /// No confirmation to expect.
    pub(crate) async fn unban(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        self.0.call_method("unban", "()", vec![ips]).await
    }

    ////////////////
    // public-api //
    ////////////////

    // Explorer (aggregated stats)

    /// summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count
    pub(crate) async fn get_status(&self) -> RpcResult<NodeStatus> {
        self.0.call_method("get_status", "NodeStatus", ()).await
    }

    pub(crate) async fn _get_cliques(&self) -> RpcResult<Vec<Clique>> {
        self.0.call_method("get_cliques", "Vec<Clique>", ()).await
    }

    // Debug (specific information)

    /// Returns the active stakers and their roll counts for the current cycle.
    pub(crate) async fn _get_stakers(&self) -> RpcResult<AddressHashMap<RollsInfo>> {
        self.0
            .call_method("get_stakers", "AddressHashMap<RollsInfo>", ())
            .await
    }

    /// Returns the algo config. // TODO: rename me please ... it hurts
    pub(crate) async fn get_algo_config(&self) -> RpcResult<AlgoConfig> {
        self.0
            .call_method("get_algo_config", "AlgoConfig", ())
            .await
    }

    /// Returns the compensation_millis.
    pub(crate) async fn get_compensation_millis(&self) -> RpcResult<i64> {
        self.0
            .call_method("get_compensation_millis", "i64", ())
            .await
    }

    /// Returns operations information associated to a given list of operations' IDs.
    pub(crate) async fn get_operations(
        &self,
        operation_ids: Vec<OperationId>,
    ) -> RpcResult<Vec<OperationInfo>> {
        self.0
            .call_method("get_operations", "Vec<OperationInfo>", vec![operation_ids])
            .await
    }

    pub(crate) async fn get_endorsements(
        &self,
        endorsement_ids: Vec<EndorsementId>,
    ) -> RpcResult<Vec<EndorsementInfo>> {
        self.0
            .call_method(
                "get_endorsements",
                "Vec<EndorsementInfo>",
                vec![endorsement_ids],
            )
            .await
    }

    /// Get information on a block given its hash
    pub(crate) async fn get_blocks(&self, block_ids: Vec<BlockId>) -> RpcResult<Vec<BlockInfo>> {
        self.0
            .call_method("get_blocks", "BlockInfo", vec![block_ids])
            .await
    }

    /// Get the block graph within the specified time interval.
    /// Optional parameters: from <time_start> (included) and to <time_end> (excluded) millisecond timestamp
    pub(crate) async fn _get_graph_interval(
        &self,
        time_interval: TimeInterval,
    ) -> RpcResult<Vec<BlockSummary>> {
        self.0
            .call_method("get_graph_interval", "Vec<BlockSummary>", time_interval)
            .await
    }

    pub(crate) async fn get_addresses(
        &self,
        addresses: Vec<Address>,
    ) -> RpcResult<Vec<AddressInfo>> {
        self.0
            .call_method("get_addresses", "Vec<AddressInfo>", vec![addresses])
            .await
    }

    // User (interaction with the node)

    /// Adds operations to pool. Returns operations that were ok and sent to pool.
    pub(crate) async fn send_operations(
        &self,
        operations: Vec<Operation>,
    ) -> RpcResult<Vec<OperationId>> {
        self.0
            .call_method("send_operations", "Vec<OperationId>", vec![operations])
            .await
    }
}
