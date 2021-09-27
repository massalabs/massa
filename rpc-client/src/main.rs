// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(str_split_whitespace_as_str)]

use crate::rpc::RpcClient;
use atty::Stream;
use cmds::Command;
use std::path::PathBuf;
use structopt::StructOpt;

mod cfg;
mod cmds;
mod repl;
mod rpc;

#[derive(StructOpt)]
struct Args {
    /// Port to listen on (Massa public API).
    #[structopt(long = "public-port", env = "PUBLIC_PORT", default_value = "33034")]
    public_port: u16,
    /// Port to listen on (Massa private API).
    #[structopt(long = "private-port", env = "PRIVATE_PORT", default_value = "33035")]
    private_port: u16,
    /// Address to listen on.
    #[structopt(short = "a", long = "address", default_value = "127.0.0.1")]
    address: String,
    /// Command that client would execute (non-interactive mode)
    #[structopt(name = "COMMAND", default_value = "Help")]
    command: Command,
    /// Optional command parameter (as a JSON parsable string)
    #[structopt(name = "PARAMETERS")]
    parameters: Vec<String>,
    /// Path of config file.
    #[structopt(
        short = "c",
        long = "config",
        parse(from_os_str),
        default_value = "config/config.toml" // FIXME: This is not used yet ...
    )]
    /// Path of wallet file.
    #[structopt(
        short = "w",
        long = "wallet",
        parse(from_os_str),
        default_value = "wallet.dat"
    )]
    config: PathBuf,
    // TODO: do we want to add more CLI args?!
    // --json
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
            let url = format!("http://{}:{}", args.address, args.private_port);
            let client = RpcClient::from_url(&url).await;
            // TODO: (de)serialize input/output from/to JSON with serde should be less verbose
            if atty::is(Stream::Stdout) {
                //////////////////////
                // Interactive mode //
                //////////////////////
                repl::run(&client).await;
            } else {
                //////////////////////////
                // Non-Interactive mode //
                //////////////////////////
                let ret = args.command.run(&client, &args.parameters).await;
                println!("{}", ret);
            }
        });
}
