// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(str_split_whitespace_as_str)]

use crate::rpc::Client;
use atty::Stream;
use cmds::Command;
use human_panic::setup_panic;
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
    setup_panic!();
    // `#[tokio::main]` macro expanded!
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let client = Client::new(&args.address, args.public_port, args.private_port).await;
            if atty::is(Stream::Stdout) {
                //////////////////////
                // Interactive mode //
                //////////////////////
                repl::run(&client).await;
            } else {
                //////////////////////////
                // Non-Interactive mode //
                //////////////////////////
                args.command.run(&client, &args.parameters).await;
            }
        });
}
