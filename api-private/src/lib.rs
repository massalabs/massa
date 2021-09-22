// Copyright (c) 2021 MASSA LABS <info@massa.net>
#![feature(async_closure)]
use api_dto::AddressInfo;
use crypto::signature::{PrivateKey, PublicKey, Signature};
use error::PrivateApiError;
use jsonrpc_core::{BoxFuture, IoHandler};
use jsonrpc_derive::rpc;
use jsonrpc_http_server::{tokio, ServerBuilder};
use models::address::{Address, AddressHashSet};
use models::node::NodeId;
use rpc_server::rpc_server;
pub use rpc_server::API;
use std::net::IpAddr;
use std::thread;

mod error;

/// Private Massa-RPC "manager mode" endpoints
#[rpc(server)]
pub trait MassaPrivate {
    fn serve_massa_private(&self);

    /// Starts the node and waits for node to start.
    /// Signals if the node is already running.
    #[rpc(name = "start_node")]
    fn start_node(&self) -> Result<(), PrivateApiError>;

    /// Gracefully stop the node.
    #[rpc(name = "stop_node")]
    fn stop_node(&self) -> Result<(), PrivateApiError>;

    #[rpc(name = "sign_message")]
    fn sign_message(
        &self,
        _: PublicKey,
        _: Vec<u8>,
    ) -> Result<(Signature, PublicKey), PrivateApiError>;

    /// Add a new private key for the node to use to stake.
    #[rpc(name = "add_staking_keys")]
    fn add_staking_keys(&self, _: Vec<PrivateKey>) -> BoxFuture<Result<(), PrivateApiError>>;

    /// Remove an address used to stake.
    #[rpc(name = "remove_staking_keys")]
    fn remove_staking_keys(&self, _: Vec<Address>) -> Result<(), PrivateApiError>;

    /// Return hashset of staking addresses.
    #[rpc(name = "list_staking_keys")]
    fn list_staking_keys(&self) -> Result<AddressHashSet, PrivateApiError>;

    #[rpc(name = "ban")]
    fn ban(&self, _: NodeId) -> Result<(), PrivateApiError>;

    #[rpc(name = "unban")]
    fn unban(&self, _: IpAddr) -> Result<(), PrivateApiError>;

    #[rpc(name = "get_addresses")]
    fn get_addresses(&self, _: Vec<Address>) -> Result<Vec<AddressInfo>, PrivateApiError>;
}

impl MassaPrivate for API {
    fn serve_massa_private(&self) {
        rpc_server!(self.clone());
    }

    fn start_node(&self) -> Result<(), PrivateApiError> {
        todo!()
    }

    fn stop_node(&self) -> Result<(), PrivateApiError> {
        todo!()
    }

    fn sign_message(
        &self,
        _: PublicKey,
        _: Vec<u8>,
    ) -> Result<(Signature, PublicKey), PrivateApiError> {
        todo!()
    }

    fn add_staking_keys(&self, keys: Vec<PrivateKey>) -> BoxFuture<Result<(), PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let x = async move || {
            Ok(cmd_sender
                .ok_or(PrivateApiError::MissingCommandSender(
                    "consensus command sender".to_string(),
                ))?
                .register_staking_private_keys(keys)
                .await?)
        };
        Box::pin(x())
    }

    fn remove_staking_keys(&self, _: Vec<Address>) -> Result<(), PrivateApiError> {
        todo!()
    }

    fn list_staking_keys(&self) -> Result<AddressHashSet, PrivateApiError> {
        todo!()
    }

    fn ban(&self, _: NodeId) -> Result<(), PrivateApiError> {
        todo!()
    }

    fn unban(&self, ip: IpAddr) -> Result<(), PrivateApiError> {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                self.network_command_sender
                    .as_ref()
                    .unwrap() // FIXME: replace by ?
                    .unban(ip)
                    .await
                    .unwrap(); // FIXME: replace by ?
                Ok(())
            })
    }

    fn get_addresses(&self, _: Vec<Address>) -> Result<Vec<AddressInfo>, PrivateApiError> {
        todo!()
    }
}
