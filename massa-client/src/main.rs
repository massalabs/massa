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
use dialoguer::Password;
use massa_sdk::Client;
use massa_wallet::Wallet;
use serde::Serialize;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use structopt::StructOpt;

mod cmds;
mod repl;
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
    /// Address to listen on
    #[structopt(long)]
    ip: Option<IpAddr>,
    /// Command that client would execute (non-interactive mode)
    #[structopt(name = "COMMAND", default_value = "help")]
    command: Command,
    /// Optional command parameter (as a JSON string)
    #[structopt(name = "PARAMETERS")]
    parameters: Vec<String>,
    /// Path of wallet file
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
    #[structopt(short = "p", long = "pwd")]
    /// Wallet password
    password: Option<String>,
}

#[derive(Serialize)]
struct JsonError {
    error: String,
}

/// Ask for the wallet password
/// If the wallet does not exist, it will require password confirmation
fn ask_password(wallet_path: &Path) -> String {
    if wallet_path.is_file() {
        Password::new()
            .with_prompt("Enter wallet password")
            .interact()
            .expect("IO error: Password reading failed, walled couldn't be unlocked")
    } else {
        Password::new()
            .with_prompt("Enter new password for wallet")
            .with_confirmation("Confirm password", "Passwords mismatching")
            .interact()
            .expect("IO error: Password reading failed, wallet couldn't be created")
    }
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

    // Setup panic handlers,
    // and when a panic occurs,
    // run default handler,
    // and then shutdown.
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    // ...
    let password = args.password.unwrap_or_else(|| ask_password(&args.wallet));
    let mut wallet = Wallet::new(args.wallet, password)?;
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
