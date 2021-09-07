// Copyright (c) 2021 MASSA LABS <info@massa.net>

use jsonrpc_core::IoHandler;
use jsonrpc_dto::{EthRpc, API};
use jsonrpc_http_server::ServerBuilder;

const URL: &str = "127.0.0.1:33035";

fn main() {
    let mut io = IoHandler::new();
    io.extend_with(API.to_delegate());

    let server = ServerBuilder::new(io)
        .start_http(&URL.parse().unwrap())
        .expect("Unable to start RPC server");

    server.wait();
}
