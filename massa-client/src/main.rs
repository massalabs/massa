// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Massa stateless CLI
#![feature(str_split_whitespace_as_str)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
use crate::settings::SETTINGS;
use anyhow::Result;
use atty::Stream;
use cmds::Command;
use console::style;
use massa_sdk::Client;
use massa_wallet::Wallet;
use serde::Serialize;
use std::{net::SocketAddr, path::PathBuf};
use structopt::StructOpt;

mod cmds;
mod repl;
mod settings;
mod utils;

#[cfg(test)]
pub mod tests;

#[derive(StructOpt)]
struct Args {
    /// Socket address to listen Massa public API.
    #[structopt(long)]
    public: Option<SocketAddr>,
    /// Socket address to listen Massa private API.
    #[structopt(long)]
    private: Option<SocketAddr>,
    /// Command that client would execute (non-interactive mode)
    #[structopt(name = "COMMAND", default_value = "help")]
    command: Command,
    /// Optional command parameter (as a JSON string)
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
    let public = match args.public {
        Some(addr) => addr,
        None => SETTINGS.default_node.public_ip,
    };
    let private = match args.private {
        Some(addr) => addr,
        None => SETTINGS.default_node.private_ip,
    };

    let client = Client::new(public, private).await;
    let mut wallet = Wallet::new(args.wallet)?;

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
