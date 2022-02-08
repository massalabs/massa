// Copyright (c) 2021 MASSA LABS <info@massa.net>

use jsonrpc_core_client::transports::http;
use jsonrpc_core_client::RpcError;
use jsonrpc_core_client::{RpcChannel, RpcResult, TypedClient};
use massa_models::api::{
    AddressInfo, BlockInfo, BlockSummary, EndorsementInfo, NodeStatus, OperationInfo, TimeInterval,
};
use massa_models::clique::Clique;
use massa_models::composite::PubkeySig;
use massa_models::prehash::{Map, Set};
use massa_models::{Address, BlockId, EndorsementId, Operation, OperationId};
use massa_signature::PrivateKey;
use std::net::{IpAddr, SocketAddr};
use tokio::time::timeout;
use tokio::time::Duration;

macro_rules! timeout {
    ($x:expr, $err_msg:expr) => {
        timeout(Duration::from_millis(10), $x) // todo set config
            .await
            .map_err(|e| RpcError::Client(format!("timeout during {}: {}", $err_msg, e)))?
    };
}
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
        match http::connect::<RpcClient>(url).await {
            Ok(client) => client,
            Err(_) => panic!("Unable to connect to Node."),
        }
    }

    /////////////////
    // private-api //
    /////////////////

    /// Gracefully stop the node.
    pub(crate) async fn stop_node(&self) -> RpcResult<()> {
        timeout!(self.0.call_method("stop_node", "()", ()), "stop node")
    }

    /// Sign message with node's key.
    /// Returns the public key that signed the message and the signature.
    pub(crate) async fn node_sign_message(&self, message: Vec<u8>) -> RpcResult<PubkeySig> {
        timeout!(
            self.0
                .call_method("node_sign_message", "PubkeySig", vec![message]),
            "node sign message"
        )
    }

    /// Add a vec of new private keys for the node to use to stake.
    /// No confirmation to expect.
    pub(crate) async fn add_staking_private_keys(
        &self,
        private_keys: Vec<PrivateKey>,
    ) -> RpcResult<()> {
        timeout!(
            self.0
                .call_method("add_staking_private_keys", "()", vec![private_keys]),
            "add staking private keys"
        )
    }

    /// Remove a vec of addresses used to stake.
    /// No confirmation to expect.
    pub(crate) async fn remove_staking_addresses(&self, addresses: Vec<Address>) -> RpcResult<()> {
        timeout!(
            self.0
                .call_method("remove_staking_addresses", "()", vec![addresses]),
            "remove staking addresses"
        )
    }

    /// Return hashset of staking addresses.
    pub(crate) async fn get_staking_addresses(&self) -> RpcResult<Set<Address>> {
        timeout!(
            self.0
                .call_method("get_staking_addresses", "Set<Address>", ()),
            "get_staking addresses"
        )
    }

    /// Bans given node id
    /// No confirmation to expect.
    pub(crate) async fn ban(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        timeout!(self.0.call_method("ban", "()", vec![ips]), "ban")
    }

    /// Unbans given ip addr
    /// No confirmation to expect.
    pub(crate) async fn unban(&self, ips: Vec<IpAddr>) -> RpcResult<()> {
        timeout!(self.0.call_method("unban", "()", vec![ips]), "unabn")
    }

    ////////////////
    // public-api //
    ////////////////

    // Explorer (aggregated stats)

    /// summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count
    pub(crate) async fn get_status(&self) -> RpcResult<NodeStatus> {
        timeout!(
            self.0.call_method("get_status", "NodeStatus", ()),
            "get status"
        )
    }

    pub(crate) async fn _get_cliques(&self) -> RpcResult<Vec<Clique>> {
        timeout!(
            self.0.call_method("get_cliques", "Vec<Clique>", ()),
            "get cliques"
        )
    }

    // Debug (specific information)

    /// Returns the active stakers and their roll counts for the current cycle.
    pub(crate) async fn _get_stakers(&self) -> RpcResult<Map<Address, u64>> {
        timeout!(
            self.0.call_method("get_stakers", "Map<Address, u64>", ()),
            "get stakers"
        )
    }

    /// Returns operations information associated to a given list of operations' IDs.
    pub(crate) async fn get_operations(
        &self,
        operation_ids: Vec<OperationId>,
    ) -> RpcResult<Vec<OperationInfo>> {
        timeout!(
            self.0
                .call_method("get_operations", "Vec<OperationInfo>", vec![operation_ids]),
            "get operations"
        )
    }

    pub(crate) async fn get_endorsements(
        &self,
        endorsement_ids: Vec<EndorsementId>,
    ) -> RpcResult<Vec<EndorsementInfo>> {
        timeout!(
            self.0.call_method(
                "get_endorsements",
                "Vec<EndorsementInfo>",
                vec![endorsement_ids],
            ),
            "get endorsements"
        )
    }

    /// Get information on a block given its BlockId
    pub(crate) async fn get_block(&self, block_id: BlockId) -> RpcResult<BlockInfo> {
        timeout!(
            self.0.call_method("get_block", "BlockInfo", vec![block_id]),
            "get block"
        )
    }

    /// Get the block graph within the specified time interval.
    /// Optional parameters: from <time_start> (included) and to <time_end> (excluded) millisecond timestamp
    pub(crate) async fn _get_graph_interval(
        &self,
        time_interval: TimeInterval,
    ) -> RpcResult<Vec<BlockSummary>> {
        timeout!(
            self.0
                .call_method("get_graph_interval", "Vec<BlockSummary>", time_interval),
            "get graph interval"
        )
    }

    pub(crate) async fn get_addresses(
        &self,
        addresses: Vec<Address>,
    ) -> RpcResult<Vec<AddressInfo>> {
        timeout!(
            self.0
                .call_method("get_addresses", "Vec<AddressInfo>", vec![addresses]),
            "get addresses"
        )
    }

    // User (interaction with the node)

    /// Adds operations to pool. Returns operations that were ok and sent to pool.
    pub(crate) async fn send_operations(
        &self,
        operations: Vec<Operation>,
    ) -> RpcResult<Vec<OperationId>> {
        timeout!(
            self.0
                .call_method("send_operations", "Vec<OperationId>", vec![operations]),
            "send operations"
        )
    }
}
