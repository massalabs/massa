// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(str_split_whitespace_as_str)]

use crate::cfg::Settings;
use crate::rpc::Client;
use atty::Stream;
use cmds::Command;
use std::net::IpAddr;
use std::path::PathBuf;
use structopt::StructOpt;
use wallet::Wallet;

mod cfg;
mod cmds;
mod repl;
mod rpc;

#[derive(StructOpt)]
struct Args {
    /// Port to listen on (Massa public API).
    #[structopt(long)]
    public_port: Option<u16>,
    /// Port to listen on (Massa private API).
    #[structopt(long)]
    private_port: Option<u16>,
    /// Address to listen on.
    #[structopt(long)]
    ip: Option<IpAddr>,
    /// Command that client would execute (non-interactive mode)
    #[structopt(name = "COMMAND", default_value = "help")]
    command: Command,
    /// Optional command parameter (as a JSON parsable string)
    #[structopt(name = "PARAMETERS")]
    parameters: Vec<String>,
    /// Path of wallet file.
    #[structopt(
        short = "w",
        long = "wallet",
        parse(from_os_str),
        default_value = "wallet.dat"
    )]
    wallet: PathBuf,
}
// TODO: use me with serde::Serialize/Deserialize traits
// #[structopt(short = "j", long = "json")]
// json: bool,

#[paw::main]
fn main(args: Args) {
    // `#[tokio::main]` macro expanded!
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // TODO: Move settings loading in another crate
            let settings = Settings::load();
            let address = match args.ip {
                Some(ip) => ip,
                None => settings.default_node.ip,
            };
            let public_port = match args.public_port {
                Some(public_port) => public_port,
                None => settings.default_node.public_port,
            };
            let private_port = match args.private_port {
                Some(private_port) => private_port,
                None => settings.default_node.private_port,
            };
            // ...
            let mut wallet = Wallet::new(args.wallet).unwrap(); // TODO
            let client = Client::new(address, public_port, private_port).await;
            if atty::is(Stream::Stdout) && args.command == Command::help {
                // Interactive mode
                repl::run(&client, &mut wallet).await;
            } else {
                // Non-Interactive mode
                println!(
                    "{}",
                    args.command
                        .run(&client, &mut wallet, &args.parameters)
                        .await
                );
            }
        });
}
