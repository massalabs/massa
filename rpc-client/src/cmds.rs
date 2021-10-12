// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::rpc::Client;
use console::style;
use crypto::signature::PrivateKey;
use models::node::NodeId;
use models::{Address, BlockId, EndorsementId, OperationId};
use std::net::IpAddr;
use std::process;
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

    // TODO:
    // #[strum(ascii_case_insensitive, message = "start a node")]
    // node_start,
    //
    #[strum(ascii_case_insensitive, message = "stops the node")]
    node_stop,

    #[strum(ascii_case_insensitive, message = "list staking addresses")]
    node_get_staking_addresses,

    #[strum(
        ascii_case_insensitive,
        props(args = "[Addresses]"),
        message = "remove staking addresses"
    )]
    node_remove_staking_addresses,

    #[strum(
        ascii_case_insensitive,
        props(args = "[PrivateKeys]"),
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
    get_addresses,

    #[strum(
        ascii_case_insensitive,
        props(args = "[BlockIds]"),
        message = "gets info about a list of blocks"
    )]
    get_blocks,

    #[strum(
        ascii_case_insensitive,
        props(args = "[EndorsementIds]"),
        message = "gets info about a list of endorsements"
    )]
    get_endorsements,

    #[strum(
        ascii_case_insensitive,
        props(args = "[OperationIds]"),
        message = "gets info about a list of operations"
    )]
    get_operations,

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

// TODO: -> Result<Box<dyn std::fmt::Display + serde::Serialize<S>>>;
type PrettyPrint = Box<dyn std::fmt::Display>;

// TODO: Should be implement as a custom Err value (for JSON output compatibility)
macro_rules! repl_err {
    ($err: expr) => {
        Box::new(style(format!("Error: {:?}", $err)).red())
    };
}

macro_rules! repl_ok {
    ($ok: expr) => {
        Box::new($ok)
    };
}

// TODO: ugly utility function
fn parse_args<T: std::str::FromStr>(args: &Vec<String>) -> Result<Vec<T>, T::Err> {
    args.iter().map(|x| x.parse::<T>()).collect()
}

// TODO: Commands could also not be APIs calls (like Wallet ones)
impl Command {
    pub(crate) fn not_found() -> PrettyPrint {
        repl_ok!("Command not found!\ntype \"help\" to get the list of commands")
    }

    pub(crate) fn _wrong_parameters(&self) -> PrettyPrint {
        repl_ok!(format!(
            "{} given is not well formed...\ntype \"help {}\" to more info",
            self.get_str("args").unwrap(),
            self.to_string()
        ))
    }

    pub(crate) fn help(&self) -> PrettyPrint {
        repl_ok!(format!(
            "- {}{}{}: {}",
            style(self.to_string()).green(),
            if self.get_str("args").is_some() {
                " "
            } else {
                ""
            },
            style(self.get_str("args").unwrap_or("")).yellow(),
            self.get_message().unwrap()
        ))
    }

    pub(crate) async fn run(&self, client: &Client, parameters: &Vec<String>) -> PrettyPrint {
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
                    repl_ok!(format!(
                        "HELP of Massa client (list of available commands):\n{}",
                        Command::iter()
                            .map(|c| format!("{}", c.help()))
                            .collect::<Vec<String>>()
                            .join("\n")
                    ))
                }
            }

            Command::unban => match parse_args::<IpAddr>(parameters) {
                Ok(ips) => match client.private.unban(ips).await {
                    Ok(_) => repl_ok!("Request of unbanning successfully sent!"),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::ban => match parse_args::<NodeId>(parameters) {
                Ok(node_ids) => match client.private.ban(node_ids[0]).await {
                    Ok(_) => repl_ok!("Request of banning successfully sent!"),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            // TODO: process spawn should be detached
            // Command::node_start => match process::Command::new("massa-node").spawn() {
            //     Ok(_) => repl_ok!("Node successfully started!"),
            //     Err(e) => repl_err!(e),
            // },
            //
            Command::node_stop => match client.private.stop_node().await {
                Ok(_) => repl_ok!("Request of stopping the Node successfully sent"),
                Err(e) => repl_err!(e),
            },

            Command::node_get_staking_addresses => {
                match client.private.get_staking_addresses().await {
                    Ok(x) => repl_ok!(format!("{:?}", x)),
                    Err(e) => repl_err!(e),
                }
            }

            Command::node_remove_staking_addresses => match parse_args::<Address>(parameters) {
                Ok(addresses) => match client.private.remove_staking_addresses(addresses).await {
                    Ok(_) => repl_ok!("Addresses successfully removed!"),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::node_add_staking_private_keys => match parse_args::<PrivateKey>(parameters) {
                Ok(private_keys) => {
                    match client.private.add_staking_private_keys(private_keys).await {
                        Ok(_) => repl_ok!("Private keys successfully added!"),
                        Err(e) => repl_err!(e),
                    }
                }
                Err(e) => repl_err!(e),
            },

            Command::node_testnet_rewards_program_ownership_proof => todo!(),

            Command::get_status => match client.public.get_status().await {
                Ok(x) => repl_ok!(x),
                Err(e) => repl_err!(e),
            },

            Command::get_addresses => match parse_args::<Address>(parameters) {
                Ok(addresses) => match client.public.get_addresses(addresses).await {
                    Ok(x) => repl_ok!(format!("{:?}", x)),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::get_blocks => match parse_args::<BlockId>(parameters) {
                Ok(block_ids) => match client.public.get_block(block_ids[0]).await {
                    Ok(x) => repl_ok!(x),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::get_endorsements => match parse_args::<EndorsementId>(parameters) {
                Ok(endorsements) => match client.public.get_endorsements(endorsements).await {
                    Ok(x) => repl_ok!(format!("{:?}", x)),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::get_operations => match parse_args::<OperationId>(parameters) {
                Ok(operations) => match client.public.get_operations(operations).await {
                    Ok(x) => repl_ok!(format!("{:?}", x)),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::wallet_info => todo!(),

            Command::wallet_add_private_keys => todo!(),

            Command::wallet_remove_addresses => todo!(),

            Command::buy_rolls => todo!(),

            Command::sell_rolls => todo!(),

            Command::send_transaction => todo!(),
        }
    }
}
