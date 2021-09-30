// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::rpc::Client;
use console::style;
use std::net::IpAddr;
use std::process;
use std::str::FromStr;
use strum::{EnumMessage, EnumProperty, IntoEnumIterator};
use strum_macros::{EnumIter, EnumMessage, EnumProperty, EnumString, ToString};

#[allow(non_camel_case_types)]
#[derive(Debug, PartialEq, EnumIter, EnumMessage, EnumString, EnumProperty, ToString)]
pub enum Command {
    #[strum(ascii_case_insensitive, message = "exit the client gracefully")]
    exit,

    #[strum(ascii_case_insensitive, message = "display this Help")]
    help,

    #[strum(
        ascii_case_insensitive,
        props(args = "IpAddr"),
        message = "unban a given IP address"
    )]
    unban,

    #[strum(
        ascii_case_insensitive,
        props(args = "NodeId"),
        message = "ban a given IP address"
    )]
    ban,

    #[strum(ascii_case_insensitive, message = "stops the node")]
    node_stop,

    #[strum(ascii_case_insensitive, message = "list staking addresses")]
    node_get_staking_addresses,

    #[strum(
        ascii_case_insensitive,
        props(args = "Address"),
        message = "remove staking addresses"
    )]
    node_remove_staking_addresses,

    #[strum(
        ascii_case_insensitive,
        props(args = "PrivateKey"),
        message = "add staking private keys"
    )]
    node_add_staking_private_keys,

    #[strum(
        ascii_case_insensitive,
        props(args = "[...]"),
        message = "generates the testnet rewards program node/staker ownership proof"
    )]
    node_testnet_rewards_program_ownership_proof,

    #[strum(
        ascii_case_insensitive,
        message = "gets the status of the node (reachable? number of peers connected, consensus, version, config parameter summaries...)"
    )]
    get_status,

    #[strum(
        ascii_case_insensitive,
        props(args = "[Addresses]"),
        message = "gets info about a list of addresses"
    )]
    get_addresses_info,

    #[strum(
        ascii_case_insensitive,
        props(args = "[BlockIds]"),
        message = "gets info about a list of blocks"
    )]
    get_blocks_info,

    #[strum(
        ascii_case_insensitive,
        props(args = "[EndorsementIds]"),
        message = "gets info about a list of endorsements"
    )]
    get_endorsements_info,

    #[strum(
        ascii_case_insensitive,
        props(args = "[OperationIds]"),
        message = "gets info about a list of operations"
    )]
    get_operations_info,

    #[strum(ascii_case_insensitive, message = "prints wallet info")]
    wallet_info,

    #[strum(
        ascii_case_insensitive,
        props(args = "[PrivateKeys]"),
        message = "adds a list of private keys to the wallet"
    )]
    wallet_add_private_keys,

    #[strum(
        ascii_case_insensitive,
        props(args = "[Addresses]"),
        message = "remove a list of addresses from the wallet"
    )]
    wallet_remove_addresses,

    #[strum(
        ascii_case_insensitive,
        props(args = "[...]"),
        message = "buy rolls with wallet address"
    )]
    buy_rolls,

    #[strum(
        ascii_case_insensitive,
        props(args = "[...]"),
        message = "sell rolls sell rolls with wallet address"
    )]
    sell_rolls,

    #[strum(ascii_case_insensitive, message = "send coins from a wallet address")]
    send_transaction,
}

macro_rules! repl_error {
    ($err: expr) => {
        style(format!("Error: {}", $err)).red().to_string()
    };
}

// TODO: Commands could also not be APIs calls (like Wallet ones)
impl Command {
    pub(crate) fn not_found() -> String {
        repl_error!("Command not found!\ntype \"help\" to get the list of commands")
    }

    pub(crate) fn help(&self) -> String {
        format!(
            "- {}{}{}: {}",
            style(self.to_string()).green(),
            if self.get_str("args").is_some() {
                " "
            } else {
                ""
            },
            style(self.get_str("args").unwrap_or("")).yellow(),
            self.get_message().unwrap()
        )
    }

    // TODO: should run(...) be impl on Command or on some struct containing clients?
    pub(crate) async fn run(&self, client: &Client, parameters: &Vec<String>, json: bool) {
        match self {
            Command::exit => process::exit(0),
            Command::help => {
                if !parameters.is_empty() {
                    if let Ok(c) = parameters[0].parse::<Command>() {
                        println!("{}", c.help());
                    } else {
                        println!("{}", Command::not_found());
                    }
                } else {
                    cli_help();
                }
            }
            Command::unban => println!(
                "{}",
                // TODO: (de)serialize input/output from/to JSON with serde should be less verbose
                match IpAddr::from_str(&parameters[0]) {
                    Ok(ip) => match &client.private.unban(&vec![ip]).await {
                        Ok(output) =>
                            if json {
                                serde_json::to_string(output)
                                    .expect("Failed to serialized command output ...")
                            } else {
                                "IP successfully unbanned!".to_string()
                            },
                        Err(e) => repl_error!(e),
                    },
                    Err(_) => repl_error!(
                        "IP given is not well formed...\ntype \"help unban\" to more info"
                    ),
                }
            ),
            Command::ban => {}
            Command::node_stop => {}
            Command::node_get_staking_addresses => {}
            Command::node_remove_staking_addresses => {}
            Command::node_add_staking_private_keys => {}
            Command::node_testnet_rewards_program_ownership_proof => {}
            Command::get_status => {}
            Command::get_addresses_info => {}
            Command::get_blocks_info => {}
            Command::get_endorsements_info => {}
            Command::get_operations_info => {}
            Command::wallet_info => {}
            Command::wallet_add_private_keys => {}
            Command::wallet_remove_addresses => {}
            Command::buy_rolls => {}
            Command::sell_rolls => {}
            Command::send_transaction => {}
        }
    }
}

fn cli_help() {
    println!("HELP of Massa client (list of available commands):");
    for c in Command::iter() {
        println!("{}", c.help());
    }
}
