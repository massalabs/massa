// Copyright (c) 2021 MASSA LABS <info@massa.net>

use jsonrpc_core::IoHandler;
use jsonrpc_derive::rpc;
use jsonrpc_http_server::ServerBuilder;
use models::amount::Amount;

/// Public Ethereum JSON-RPC endpoints (in intend to be compatible with EthRpc.io)
#[rpc(server)]
pub trait EthRpc {
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

pub struct API;

impl EthRpc for API {
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

pub fn serve(url: &str) {
    let mut io = IoHandler::new();
    io.extend_with(API.to_delegate());

    let server = ServerBuilder::new(io)
        .start_http(&url.parse().unwrap())
        .expect("Unable to start RPC server");

    server.wait();
}
