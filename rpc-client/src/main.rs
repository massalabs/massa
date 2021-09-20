// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::rpc::RpcClient;
use atty::Stream;
use cmds::Command;
use std::path::PathBuf;
use structopt::StructOpt;

mod cmds;
mod rpc;

#[derive(StructOpt)]
struct Args {
    /// Port to listen on (Massa public API).
    #[structopt(short = "p", long = "port", env = "PORT", default_value = "33035")]
    port: u16,
    /// Port to listen on (Massa private API).
    #[structopt(
        short = "pp",
        long = "private-port",
        env = "PRIVATE_PORT",
        default_value = "33034"
    )]
    _private_port: u16,
    /// Address to listen on.
    #[structopt(short = "a", long = "address", default_value = "127.0.0.1")]
    address: String,
    /// Command that client would execute (non-interactive mode)
    #[structopt(short = "cmd", long = "command", default_value = "InteractiveMode")]
    command: Command,
    /// Optional command parameter (as a JSON parsable string)
    #[structopt(short = "p", long = "parameters", default_value = "{}")]
    parameters: String,
    /// Path of config file.
    #[structopt(
        short = "cfg",
        long = "config",
        parse(from_os_str),
        default_value = "config/config.toml"
    )]
    _config: PathBuf,
    // TODO: do we want to add more CLI args?!
}

#[paw::main]
fn main(args: Args) {
    // `#[tokio::main]` macro expanded!
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // TODO: We should handle 2 different ports
            let url = format!("http://{}:{}", args.address, args.port);
            let client = RpcClient::from_url(&url).await;
            // TODO: (de)serialize input/output from/to JSON with serde should be less verbose
            if atty::is(Stream::Stdout) && args.command == Command::InteractiveMode {
                //////////////////////
                // Interactive mode //
                //////////////////////
            } else {
                //////////////////////////
                // Non-Interactive mode //
                //////////////////////////
                println!("{}", args.command.run(client, &args.parameters).await);
            }
        });
}
