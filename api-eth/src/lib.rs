// Copyright (c) 2021 MASSA LABS <info@massa.net>

use jsonrpc_core::IoHandler;
use jsonrpc_derive::rpc;
use jsonrpc_http_server::ServerBuilder;
use models::amount::Amount;
use rpc_server::rpc_server;
pub use rpc_server::API;
use std::thread;

/// Public Ethereum JSON-RPC endpoints (in intend to be compatible with EthRpc.io)
#[rpc(server)]
pub trait EthRpc {
    fn serve_eth_rpc(&self); // todo add needed command servers

    /// Will be implemented later, when smart contracts are added.
    #[rpc(name = "Call")]
    fn call(&self) -> jsonrpc_core::Result<()>;

    #[rpc(name = "getBalance")]
    fn get_balance(&self) -> jsonrpc_core::Result<Amount>;

    #[rpc(name = "sendTransaction")]
    fn send_transaction(&self) -> jsonrpc_core::Result<()>;

    #[rpc(name = "HelloWorld")]
    fn hello_world(&self) -> jsonrpc_core::Result<String>;
}

impl EthRpc for API {
    fn serve_eth_rpc(&self) {
        // todo add needed command servers
        rpc_server!(self.clone());
    }

    fn call(&self) -> jsonrpc_core::Result<()> {
        todo!()
    }

    fn get_balance(&self) -> jsonrpc_core::Result<Amount> {
        todo!()
    }

    fn send_transaction(&self) -> jsonrpc_core::Result<()> {
        todo!()
    }

    fn hello_world(&self) -> jsonrpc_core::Result<String> {
        Ok(String::from("Hello, World!"))
    }
}
