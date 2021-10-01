// Copyright (c) 2021 MASSA LABS <info@massa.net>

use jsonrpc_core_client::transports::http;
use jsonrpc_core_client::{RpcChannel, RpcResult, TypedClient};
use std::net::IpAddr;
use models::address::AddressHashSet;
use models::Address;
use models::node::NodeId;
use crypto::signature::{PublicKey, Signature, PrivateKey};

// TODO: This crate should at some point be renamed `client`, `massa` or `massa-client`
// and replace the previous one!

pub struct Client {
    pub public: RpcClient,
    pub private: RpcClient,
}

impl Client {
    pub(crate) async fn new(address: &str, public_port: u16, private_port: u16) -> Client {
        let public_url = format!("http://{}:{}", address, public_port);
        let private_url = format!("http://{}:{}", address, private_port);
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

    /// Starts the node and waits for node to start.
    /// Signals if the node is already running.
    pub(crate) async fn start_node(&self) -> RpcResult<()> {
        self.0.call_method("start_node", "()", ()).await
    }

    /// Gracefully stop the node.
    pub(crate) async fn stop_node(&self) -> RpcResult<()> {
        self.0.call_method("stop_node", "()", ()).await
    }

    /// Sign message with node's key.
    /// Returns the public key that signed the message and the signature.
    pub(crate) async fn node_sign_message(
        &self,
        message: Vec<u8>,
    ) -> RpcResult<(PublicKey, Signature)> {
        self.0
            .call_method("node_sign_message", "(PublicKey, Signature)", message)
            .await
    }

    /// Add a vec of new private keys for the node to use to stake.
    /// No confirmation to expect.
    pub(crate) async fn add_staking_keys(&self, private_keys: Vec<PrivateKey>) -> RpcResult<()> {
        self.0
            .call_method("add_staking_keys", "()", private_keys)
            .await
    }

    /// Remove a vec of addresses used to stake.
    /// No confirmation to expect.
    pub(crate) async fn remove_staking_keys(&self, addresses: Vec<Address>) -> RpcResult<()> {
        self.0
            .call_method("remove_staking_keys", "()", addresses)
            .await
    }

    /// Return hashset of staking addresses.
    pub(crate) async fn list_staking_keys(&self) -> RpcResult<AddressHashSet> {
        self.0
            .call_method("list_staking_keys", "AddressHashSet", ())
            .await
    }

    /// Bans given node id
    /// No confirmation to expect.
    pub(crate) async fn ban(&self, node_id: NodeId) -> RpcResult<()> {
        self.0.call_method("ban", "()", node_id).await
    }

    /// Unbans given ip addr
    /// No confirmation to expect.
    pub(crate) async fn unban(&self, ip: &Vec<IpAddr>) -> RpcResult<()> {
        self.0.call_method("unban", "()", ip).await
    }
}
