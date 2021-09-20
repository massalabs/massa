// Copyright (c) 2021 MASSA LABS <info@massa.net>

use atty::Stream;
// use interact_prompt::Settings;
use jsonrpc_core_client::transports::http;
use jsonrpc_core_client::{RpcChannel, RpcResult, TypedClient};
use std::net::IpAddr;
use structopt::StructOpt;

// TODO: This crate should at some point be renamed `client`, `massa` or `massa-client` and replace the previous one!

// TODO: Did we crate 2 RpcClient structs? (to separate public/private calls in impl)
struct RpcClient(TypedClient);

impl From<RpcChannel> for RpcClient {
    fn from(channel: RpcChannel) -> Self {
        RpcClient(channel.into())
    }
}

impl RpcClient {
    // TODO: This is for test purpose only ond should be removed
    // We should here implement all of our desired API calls
    async fn hello_world(&self) -> RpcResult<String> {
        self.0.call_method("HelloWorld", "String", ()).await
    }

    // End-to-end example with `unban` command:
    async fn unban(&self, ip: IpAddr) -> RpcResult<()> {
        self.0.call_method("unban", "String", ip).await
    }
}

#[derive(StructOpt)]
struct Args {
    /// Port to listen on.
    #[structopt(short = "p", long = "port", env = "PORT", default_value = "33035")]
    port: u16,
    /// Address to listen on.
    #[structopt(short = "a", long = "address", default_value = "127.0.0.1")]
    address: String,
    // TODO:
    // - command: Option<Enum>, if it's None -> go to interactive mode
    // - config: file path
    // - ?
}

enum Command {
    HelloWorld,
    Unban,
    // TODO: write the full list of command
}

#[paw::main]
fn main(args: Args) {
    if !atty::is(Stream::Stdout) {
        // non-interactive mode
    } else {
        // TODO: fancy ASCII art that welcome user :)
        // TODO: interact_prompt::direct(Settings::default(), ()).unwrap();
    }

    // TODO: this code snippet should be part of a `Command.run()` trait?
    // Commands could also not be APIs calls (like Wallet ones)
    let url = format!("http://{}:{}", args.address, args.port);
    let res = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let client = http::connect::<RpcClient>(&url).await.unwrap();
            client.hello_world().await.unwrap()
        });
    println!("{}", res); // TODO: serialize output to JSON with serde in non-interactive mode
}
