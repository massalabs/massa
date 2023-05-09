// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Massa stateless CLI
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
use crate::settings::SETTINGS;
use anyhow::Result;
use atty::Stream;
use cmds::Command;
use console::style;
use dialoguer::Password;
use massa_sdk::{Client, ClientConfig, HttpConfig};
use massa_wallet::Wallet;
use serde::Serialize;
use std::env;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use structopt::StructOpt;

mod cmds;
mod display;
mod repl;
mod settings;

#[cfg(test)]
pub(crate)  mod tests;

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
pub(crate) fn ask_password(wallet_path: &Path) -> String {
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
fn main(args: Args) -> anyhow::Result<()> {
    let tokio_rt = tokio::runtime::Builder::new_multi_thread()
        .thread_name_fn(|| {
            static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
            format!("tokio-client-{}", id)
        })
        .enable_all()
        .build()
        .unwrap();

    tokio_rt.block_on(run(args))
}

async fn run(args: Args) -> Result<()> {
    let client_config = ClientConfig {
        max_request_body_size: SETTINGS.client.max_request_body_size,
        request_timeout: SETTINGS.client.request_timeout,
        max_concurrent_requests: SETTINGS.client.max_concurrent_requests,
        certificate_store: SETTINGS.client.certificate_store.clone(),
        id_kind: SETTINGS.client.id_kind.clone(),
        max_log_length: SETTINGS.client.max_log_length,
        headers: SETTINGS.client.headers.clone(),
    };

    let http_config = HttpConfig {
        client_config,
        enabled: SETTINGS.client.http.enabled,
    };

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

    let client = Client::new(address, public_port, private_port, &http_config).await;
    if atty::is(Stream::Stdout) && args.command == Command::help && !args.json {
        // Interactive mode
        repl::run(&client, &args.wallet, args.password).await?;
    } else {
        // Non-Interactive mode

        // Only prompt for password if the command needs wallet access.
        let mut wallet_opt = match args.command.is_pwd_needed() {
            true => {
                let password = match (args.password, env::var("MASSA_CLIENT_PASSWORD")) {
                    (Some(pwd), _) => pwd,
                    (_, Ok(pwd)) => pwd,
                    _ => ask_password(&args.wallet),
                };

                let wallet = Wallet::new(args.wallet, password)?;
                Some(wallet)
            }
            false => None,
        };

        match args
            .command
            .run(&client, &mut wallet_opt, &args.parameters, args.json)
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
