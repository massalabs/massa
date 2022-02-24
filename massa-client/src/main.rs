// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![feature(str_split_whitespace_as_str)]

use crate::rpc::Client;
use crate::settings::SETTINGS;
use anyhow::Result;
use atty::Stream;
use cmds::Command;
use console::style;
use massa_wallet::Wallet;
use serde::Serialize;
use std::net::IpAddr;
use std::path::PathBuf;
use structopt::StructOpt;

mod cmds;
mod repl;
mod rpc;
mod settings;
mod utils;

#[cfg(test)]
pub mod tests;

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
    /// Enable a mode where input/output are serialized as JSON
    #[structopt(short = "j", long = "json")]
    json: bool,
}

#[derive(Serialize)]
struct JsonError {
    error: String,
}

#[paw::main]
#[tokio::main]
async fn main(args: Args) -> Result<()> {
    // TODO: move settings loading in another crate ... see #1277
    let settings = SETTINGS.clone();
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
    let mut wallet = Wallet::new(args.wallet)?;
    let client = Client::new(address, public_port, private_port).await;
    if atty::is(Stream::Stdout) && args.command == Command::help && !args.json {
        // Interactive mode
        repl::run(&client, &mut wallet).await;
    } else {
        // Non-Interactive mode
        match args
            .command
            .run(&client, &mut wallet, &args.parameters, args.json)
            .await
        {
            Ok(output) => {
                if args.json {
                    output
                        .stdout_json()
                        .expect("fail to serialize to JSON command output")
                } else {
                    output.pretty_print();
                }
            }
            Err(e) => {
                if args.json {
                    let error = serde_json::to_string(&JsonError {
                        error: format!("{:?}", e),
                    })
                    .expect("fail to serialize to JSON error");
                    println!("{}", error);
                } else {
                    println!("{}", style(format!("Error: {}", e)).red());
                }
            }
        }
    }
    Ok(())
}
