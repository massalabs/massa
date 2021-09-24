// Copyright (c) 2021 MASSA LABS <info@massa.net>
#![feature(async_closure)]
use communication::network::NetworkCommandSender;
use consensus::{ConsensusCommandSender, ConsensusConfig};
use crypto::signature::{PrivateKey, PublicKey, Signature};
use error::PrivateApiError;
use jsonrpc_core::{BoxFuture, IoHandler};
use jsonrpc_derive::rpc;
use jsonrpc_http_server::ServerBuilder;
use models::address::{Address, AddressHashSet};
use models::node::NodeId;
use rpc_server::APIConfig;
pub use rpc_server::API;
use std::net::IpAddr;
use std::thread;
use tokio::sync::mpsc;

mod error;

pub struct ApiMassaPrivate {
    pub url: String,
    pub consensus_command_sender: ConsensusCommandSender,
    pub network_command_sender: NetworkCommandSender,
    pub consensus_config: ConsensusConfig,
    pub api_config: APIConfig,
    pub stop_node_channel: mpsc::Sender<()>,
}

/// Private Massa-RPC "manager mode" endpoints
#[rpc(server)]
pub trait MassaPrivate {
    /// Starts the node and waits for node to start.
    /// Signals if the node is already running.
    #[rpc(name = "start_node")]
    fn start_node(&self) -> Result<(), PrivateApiError>;

    /// Gracefully stop the node.
    #[rpc(name = "stop_node")]
    fn stop_node(&self) -> BoxFuture<Result<(), PrivateApiError>>;

    /// Sign message with node's key.
    /// Returns the public key that signed the message and the signature.
    #[rpc(name = "node_sign_message")]
    fn node_sign_message(
        &self,
        _: Vec<u8>,
    ) -> BoxFuture<Result<(PublicKey, Signature), PrivateApiError>>;

    /// Add a vec of new private keys for the node to use to stake.
    /// No confirmation to expect.
    #[rpc(name = "add_staking_keys")]
    fn add_staking_keys(&self, _: Vec<PrivateKey>) -> BoxFuture<Result<(), PrivateApiError>>;

    /// Remove a vec of addresses used to stake.
    /// No confirmation to expect.
    #[rpc(name = "remove_staking_keys")]
    fn remove_staking_keys(&self, _: Vec<Address>) -> BoxFuture<Result<(), PrivateApiError>>;

    /// Return hashset of staking addresses.
    #[rpc(name = "list_staking_keys")]
    fn list_staking_keys(&self) -> BoxFuture<Result<AddressHashSet, PrivateApiError>>;

    /// Bans given node id
    /// No confirmation to expect.
    #[rpc(name = "ban")]
    fn ban(&self, _: NodeId) -> BoxFuture<Result<(), PrivateApiError>>;

    /// Unbans given ip addr
    /// No confirmation to expect.
    #[rpc(name = "unban")]
    fn unban(&self, _: IpAddr) -> BoxFuture<Result<(), PrivateApiError>>;
}

impl ApiMassaPrivate {
    /// creates Private Api from url and need command senders and configs
    pub fn create(
        url: &str,
        consensus_command_sender: ConsensusCommandSender,
        network_command_sender: NetworkCommandSender,
        api_config: APIConfig,
        consensus_config: ConsensusConfig,
    ) -> (Self, mpsc::Receiver<()>) {
        let (stop_node_channel, rx) = mpsc::channel(1);
        (
            ApiMassaPrivate {
                url: url.to_string(),
                consensus_command_sender,
                network_command_sender,
                consensus_config,
                api_config,
                stop_node_channel: stop_node_channel,
            },
            rx,
        )
    }

    /// Starts massa private server.
    pub fn serve_massa_private(self) {
        let mut io = IoHandler::new();
        let url = self.url.parse().unwrap();
        io.extend_with(self.to_delegate());

        let server = ServerBuilder::new(io)
            .start_http(&url)
            .expect("Unable to start RPC server");

        thread::spawn(|| server.wait());
    }
}

impl MassaPrivate for ApiMassaPrivate {
    fn start_node(&self) -> Result<(), PrivateApiError> {
        todo!()
    }

    fn stop_node(&self) -> BoxFuture<Result<(), PrivateApiError>> {
        let stop = self.stop_node_channel.clone();
        let closure = async move || {
            stop.send(()).await.map_err(|e| {
                PrivateApiError::SendChannelError(format!("error sending stop signal {:?}", e))
            })?;
            Ok(())
        };

        Box::pin(closure())
    }

    fn node_sign_message(
        &self,
        message: Vec<u8>,
    ) -> BoxFuture<Result<(PublicKey, Signature), PrivateApiError>> {
        let network_command_sender = self.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.node_sign_message(message).await?);
        Box::pin(closure())
    }

    fn add_staking_keys(&self, keys: Vec<PrivateKey>) -> BoxFuture<Result<(), PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let closure = async move || Ok(cmd_sender.register_staking_private_keys(keys).await?);
        Box::pin(closure())
    }

    fn remove_staking_keys(&self, keys: Vec<Address>) -> BoxFuture<Result<(), PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let closure = async move || {
            Ok(cmd_sender
                .remove_staking_addresses(keys.into_iter().collect())
                .await?)
        };
        Box::pin(closure())
    }

    fn list_staking_keys(&self) -> BoxFuture<Result<AddressHashSet, PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let closure = async move || Ok(cmd_sender.get_staking_addresses().await?);
        Box::pin(closure())
    }

    fn ban(&self, node_id: NodeId) -> BoxFuture<Result<(), PrivateApiError>> {
        let network_command_sender = self.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.ban(node_id).await?);
        Box::pin(closure())
    }

    fn unban(&self, ip: IpAddr) -> BoxFuture<Result<(), PrivateApiError>> {
        let network_command_sender = self.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.unban(ip).await?);
        Box::pin(closure())
    }
}
