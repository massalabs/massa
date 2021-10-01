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

    #[strum(ascii_case_insensitive, message = "start a node")]
    node_start,

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

macro_rules! repl_err {
    ($err: expr) => {
        style(format!("Error: {}", $err)).red().to_string()
    };
}

macro_rules! repl_ok {
    ($ok: expr) => {
        $ok.to_string()
    };
}

// TODO: Commands could also not be APIs calls (like Wallet ones)
impl Command {
    pub(crate) fn not_found() -> String {
        repl_err!("Command not found!\ntype \"help\" to get the list of commands")
    }

    pub(crate) fn wrong_parameters(&self) -> String {
        repl_err!(format!(
            "{} given is not well formed...\ntype \"help {}\" to more info",
            self.get_str("args").unwrap(),
            self.to_string()
        ))
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

    // TODO: Return type should be something like:
    // use std::fmt::Display;
    // use serde_json::ser::Formatter;
    // pub(crate) async fn run<T: Display + Formatter>(&self, client: &Client, parameters: &Vec<String>) -> T
    pub(crate) async fn run(&self, client: &Client, parameters: &Vec<String>) -> String {
        match self {
            Command::exit => process::exit(0),

            Command::help => {
                if !parameters.is_empty() {
                    if let Ok(c) = parameters[0].parse::<Command>() {
                        c.help()
                    } else {
                        Command::not_found()
                    }
                } else {
                    format!(
                        "HELP of Massa client (list of available commands):\n{}",
                        Command::iter()
                            .map(|c| c.help())
                            .collect::<Vec<String>>()
                            .join("\n")
                    )
                }
            }

            Command::unban => match IpAddr::from_str(&parameters[0]) {
                Ok(ip) => match &client.private.unban(&vec![ip]).await {
                    Ok(_) => repl_ok!("Request of unbanning successfully sent!"),
                    Err(e) => repl_err!(e),
                },
                Err(_) => self.wrong_parameters(),
            },

            Command::ban => match serde_json::from_str(&parameters[0]) {
                Ok(node_id) => match &client.private.ban(node_id).await {
                    Ok(_) => repl_ok!("Request of banning successfully sent!"),
                    Err(e) => repl_err!(e),
                },
                Err(_) => self.wrong_parameters(),
            },

            Command::node_start => match process::Command::new("massa-node").spawn() {
                Ok(_) => repl_ok!("Node successfully started!"),
                Err(e) => repl_err!(e),
            },

            Command::node_stop => match &client.private.stop_node().await {
                Ok(_) => repl_ok!("Request of stopping the Node successfully sent"),
                Err(e) => repl_err!(e),
            },

            Command::node_get_staking_addresses => {
                match &client.private.get_staking_addresses().await {
                    Ok(output) => serde_json::to_string(output)
                        .expect("Failed to serialized command output ..."),
                    Err(e) => repl_err!(e),
                }
            }

            Command::node_remove_staking_addresses => match serde_json::from_str(&parameters[0]) {
                Ok(addresses) => match &client.private.remove_staking_addresses(addresses).await {
                    Ok(_) => repl_ok!("Addresses successfully removed!"),
                    Err(e) => repl_err!(e),
                },
                Err(_) => self.wrong_parameters(),
            },

            Command::node_add_staking_private_keys => match serde_json::from_str(&parameters[0]) {
                Ok(private_keys) => {
                    match &client.private.add_staking_private_keys(private_keys).await {
                        Ok(_) => repl_ok!("Private keys successfully added!"),
                        Err(e) => repl_err!(e),
                    }
                }
                Err(_) => self.wrong_parameters(),
            },

            Command::node_testnet_rewards_program_ownership_proof => {
                todo!()
            }

            Command::get_status => {
                todo!()
            }

            Command::get_addresses_info => {
                todo!()
            }

            Command::get_blocks_info => {
                todo!()
            }

            Command::get_endorsements_info => {
                todo!()
            }

            Command::get_operations_info => {
                todo!()
            }

            Command::wallet_info => {
                todo!()
            }

            Command::wallet_add_private_keys => {
                todo!()
            }

            Command::wallet_remove_addresses => {
                todo!()
            }

            Command::buy_rolls => {
                todo!()
            }

            Command::sell_rolls => {
                todo!()
            }

            Command::send_transaction => {
                todo!()
            }
        }
    }
}
