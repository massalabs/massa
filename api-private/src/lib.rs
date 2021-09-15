// Copyright (c) 2021 MASSA LABS <info@massa.net>

use api_dto::AddressInfo;
use crypto::signature::{PrivateKey, PublicKey, Signature};
use jsonrpc_core::IoHandler;
use jsonrpc_derive::rpc;
use jsonrpc_http_server::ServerBuilder;
use models::address::{Address, AddressHashSet};
use models::node::NodeId;
use std::net::IpAddr;

/// Private Massa-RPC "manager mode" endpoints
#[rpc(server)]
pub trait MassaPrivate {
    /// Starts the node and waits for node to start.
    /// Signals if the node is already running.
    #[rpc(name = "start_node")]
    fn start_node(&self) -> jsonrpc_core::Result<()>;

    /// Gracefully stop the node.
    #[rpc(name = "stop_node")]
    fn stop_node(&self) -> jsonrpc_core::Result<()>;

    #[rpc(name = "sign_message")]
    fn sign_message(&self, _: Vec<u8>) -> jsonrpc_core::Result<(Signature, PublicKey)>;

    /// Add a new private key for the node to use to stake.
    #[rpc(name = "add_staking_keys")]
    fn add_staking_keys(&self, _: Vec<PrivateKey>) -> jsonrpc_core::Result<()>;

    /// Remove an address used to stake.
    #[rpc(name = "remove_staking_keys")]
    fn remove_staking_keys(&self, _: Vec<Address>) -> jsonrpc_core::Result<()>;

    /// Return hashset of staking addresses.
    #[rpc(name = "list_staking_keys")]
    fn list_staking_keys(&self) -> jsonrpc_core::Result<AddressHashSet>;

    #[rpc(name = "ban")]
    fn ban(&self, _: NodeId) -> jsonrpc_core::Result<()>;

    #[rpc(name = "unban")]
    fn unban(&self, _: IpAddr) -> jsonrpc_core::Result<()>;

    #[rpc(name = "get_addresses")]
    fn get_addresses(&self, _: Vec<Address>) -> jsonrpc_core::Result<Vec<AddressInfo>>;
}

pub struct API;

impl MassaPrivate for API {
    fn start_node(&self) -> jsonrpc_core::Result<()> {
        todo!()
    }

    fn stop_node(&self) -> jsonrpc_core::Result<()> {
        todo!()
    }

    fn sign_message(&self, _: Vec<u8>) -> jsonrpc_core::Result<(Signature, PublicKey)> {
        todo!()
    }

    fn add_staking_keys(&self, _: Vec<PrivateKey>) -> jsonrpc_core::Result<()> {
        todo!()
    }

    fn remove_staking_keys(&self, _: Vec<Address>) -> jsonrpc_core::Result<()> {
        todo!()
    }

    fn list_staking_keys(&self) -> jsonrpc_core::Result<AddressHashSet> {
        todo!()
    }

    fn ban(&self, _: NodeId) -> jsonrpc_core::Result<()> {
        todo!()
    }

    fn unban(&self, _: IpAddr) -> jsonrpc_core::Result<()> {
        todo!()
    }

    fn get_addresses(&self, _: Vec<Address>) -> jsonrpc_core::Result<Vec<AddressInfo>> {
        todo!()
    }
}

pub fn serve(url: &str) {
    let mut io = IoHandler::new();
    io.extend_with(API.to_delegate());

    let server = ServerBuilder::new(io)
        .start_http(&url.parse().unwrap())
        .expect("Unable to start RPC server");

    server.wait();
}
