// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::rpc::Client;
use api_dto::{AddressInfo, EndorsementInfo, OperationInfo};
use console::style;
use crypto::signature::PrivateKey;
use models::node::NodeId;
use models::timeslots::get_current_latest_block_slot;
use models::{
    Address, Amount, BlockId, EndorsementId, OperationContent, OperationId, OperationType, Slot,
};
use std::net::IpAddr;
use std::process;
use strum::{EnumMessage, EnumProperty, IntoEnumIterator};
use strum_macros::{EnumIter, EnumMessage, EnumProperty, EnumString, ToString};
use wallet::Wallet;

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

// TODO: ugly utilities functions
fn parse_vec<T: std::str::FromStr>(args: &Vec<String>) -> Result<Vec<T>, T::Err> {
    args.iter().map(|x| x.parse::<T>()).collect()
}

fn format_vec<T: std::fmt::Display>(output: &Vec<T>) -> String {
    output
        .iter()
        .map(|x| format!("{}", x))
        .collect::<Vec<String>>()
        .join("\n")
}

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

    pub(crate) async fn run(
        &self,
        client: &Client,
        wallet: &mut Wallet,
        parameters: &Vec<String>,
    ) -> PrettyPrint {
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

            Command::unban => match parse_vec::<IpAddr>(parameters) {
                Ok(ips) => match client.private.unban(ips).await {
                    Ok(_) => repl_ok!("Request of unbanning successfully sent!"),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::ban => match parse_vec::<NodeId>(parameters) {
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
                    Ok(x) => repl_ok!(format!("{:?}", x)), // TODO
                    Err(e) => repl_err!(e),
                }
            }

            Command::node_remove_staking_addresses => match parse_vec::<Address>(parameters) {
                Ok(addresses) => match client.private.remove_staking_addresses(addresses).await {
                    Ok(_) => repl_ok!("Addresses successfully removed!"),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::node_add_staking_private_keys => match parse_vec::<PrivateKey>(parameters) {
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

            Command::get_addresses => match parse_vec::<Address>(parameters) {
                Ok(addresses) => match client.public.get_addresses(addresses).await {
                    Ok(x) => repl_ok!(format_vec::<AddressInfo>(&x)),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::get_blocks => match parse_vec::<BlockId>(parameters) {
                Ok(block_ids) => match client.public.get_block(block_ids[0]).await {
                    Ok(x) => repl_ok!(x),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::get_endorsements => match parse_vec::<EndorsementId>(parameters) {
                Ok(endorsements) => match client.public.get_endorsements(endorsements).await {
                    Ok(x) => repl_ok!(format_vec::<EndorsementInfo>(&x)),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::get_operations => match parse_vec::<OperationId>(parameters) {
                Ok(operations) => match client.public.get_operations(operations).await {
                    Ok(x) => repl_ok!(format_vec::<OperationInfo>(&x)),
                    Err(e) => repl_err!(e),
                },
                Err(e) => repl_err!(e),
            },

            Command::wallet_info => {
                let addrs = wallet.get_full_wallet().keys().copied().collect();
                // TODO: maybe too much info in wallet_info
                match client.public.get_addresses(addrs).await {
                    Ok(x) => repl_ok!(format!("{:?}", x)),
                    Err(e) => repl_err!(e),
                }
            }

            Command::wallet_add_private_keys => match parse_vec::<PrivateKey>(parameters) {
                Ok(x) => {
                    let mut res = "".to_string();
                    for key in x.into_iter() {
                        match wallet.add_private_key(key) {
                            Ok(ad) => {
                                res.push_str(&format!(
                                    "Derived and added address {:?} to the wallet\n",
                                    ad
                                ));
                            }
                            Err(e) => return repl_err!(e),
                        }
                    }
                    repl_ok!(res)
                }
                Err(e) => repl_err!(e),
            },

            Command::wallet_remove_addresses => match parse_vec::<Address>(parameters) {
                Ok(x) => {
                    let mut res = "".to_string();
                    for key in x.into_iter() {
                        match wallet.remove_address(key) {
                            Some(_) => {
                                res.push_str(&format!("Removed address {:?} to the wallet\n", key));
                            }
                            None => {
                                res.push_str(&format!("Address {:?} wasn't in the wallet\n", key));
                            }
                        }
                    }
                    repl_ok!(res)
                }
                Err(e) => repl_err!(e),
            },

            Command::buy_rolls => {
                if parameters.len() != 3 {
                    return repl_err!("Wrong param numbers"); // TODO: print help buy roll
                } else {
                    let addr = match parameters[0].parse::<Address>() {
                        Ok(a) => a,
                        Err(e) => return repl_err!(e),
                    };
                    let roll_count = match parameters[1].parse::<u64>() {
                        Ok(a) => a,
                        Err(e) => return repl_err!(e),
                    };
                    let fee = match parameters[2].parse::<Amount>() {
                        Ok(a) => a,
                        Err(e) => return repl_err!(e),
                    };

                    let op = OperationType::RollBuy { roll_count };

                    let cfg = match client.public.get_algo_config().await {
                        Ok(x) => x,
                        Err(e) => return repl_err!(e),
                    };

                    let compensation_millis = match client.public.get_compensation_millis().await {
                        Ok(x) => x,
                        Err(e) => return repl_err!(e),
                    };

                    let slot = match get_current_latest_block_slot(
                        cfg.thread_count,
                        cfg.t0,
                        cfg.genesis_timestamp,
                        compensation_millis,
                    ) {
                        Ok(a) => a.unwrap_or_else(|| Slot::new(0, 0)),
                        Err(e) => return repl_err!(e),
                    };

                    let mut expire_period = slot.period + cfg.operation_validity_periods;
                    if slot.thread >= addr.get_thread(cfg.thread_count) {
                        expire_period += 1;
                    };
                    let sender_public_key = match wallet.find_associated_public_key(addr) {
                        Some(pk) => *pk,
                        None => return repl_err!("Missing public key"),
                    };

                    let content = OperationContent {
                        sender_public_key,
                        fee,
                        expire_period,
                        op,
                    };
                    let op = match wallet.create_operation(content, addr) {
                        Ok(op) => op,
                        Err(e) => return repl_err!(e),
                    };

                    match client.public.send_operations(vec![op]).await {
                        Ok(x) => repl_ok!(format!("Sent operation id : {:?}", x)),
                        Err(e) => repl_err!(e),
                    }
                }
            }

            Command::sell_rolls => {
                if parameters.len() != 3 {
                    return repl_err!("Wrong param numbers"); // TODO: print help sell roll
                } else {
                    let addr = match parameters[0].parse::<Address>() {
                        Ok(a) => a,
                        Err(e) => return repl_err!(e),
                    };
                    let roll_count = match parameters[1].parse::<u64>() {
                        Ok(a) => a,
                        Err(e) => return repl_err!(e),
                    };
                    let fee = match parameters[2].parse::<Amount>() {
                        Ok(a) => a,
                        Err(e) => return repl_err!(e),
                    };

                    let op = OperationType::RollSell { roll_count };

                    let cfg = match client.public.get_algo_config().await {
                        Ok(x) => x,
                        Err(e) => return repl_err!(e),
                    };

                    let compensation_millis = match client.public.get_compensation_millis().await {
                        Ok(x) => x,
                        Err(e) => return repl_err!(e),
                    };

                    let slot = match get_current_latest_block_slot(
                        cfg.thread_count,
                        cfg.t0,
                        cfg.genesis_timestamp,
                        compensation_millis,
                    ) {
                        Ok(a) => a.unwrap_or_else(|| Slot::new(0, 0)),
                        Err(e) => return repl_err!(e),
                    };

                    let mut expire_period = slot.period + cfg.operation_validity_periods;
                    if slot.thread >= addr.get_thread(cfg.thread_count) {
                        expire_period += 1;
                    };
                    let sender_public_key = match wallet.find_associated_public_key(addr) {
                        Some(pk) => *pk,
                        None => return repl_err!("Missing public key"),
                    };

                    let content = OperationContent {
                        sender_public_key,
                        fee,
                        expire_period,
                        op,
                    };
                    let op = match wallet.create_operation(content, addr) {
                        Ok(op) => op,
                        Err(e) => return repl_err!(e),
                    };

                    match client.public.send_operations(vec![op]).await {
                        Ok(x) => repl_ok!(format!("Sent operation id : {:?}", x)),
                        Err(e) => repl_err!(e),
                    }
                }
            }

            Command::send_transaction => {
                if parameters.len() != 4 {
                    return repl_err!("Wrong param numbers"); // TODO: print help transaction
                } else {
                    let addr = match parameters[0].parse::<Address>() {
                        Ok(a) => a,
                        Err(e) => return repl_err!(e),
                    };
                    let recipient_address = match parameters[1].parse::<Address>() {
                        Ok(a) => a,
                        Err(e) => return repl_err!(e),
                    };

                    let amount = match parameters[2].parse::<Amount>() {
                        Ok(a) => a,
                        Err(e) => return repl_err!(e),
                    };

                    let fee = match parameters[3].parse::<Amount>() {
                        Ok(a) => a,
                        Err(e) => return repl_err!(e),
                    };

                    let op = OperationType::Transaction {
                        recipient_address,
                        amount,
                    };

                    let cfg = match client.public.get_algo_config().await {
                        Ok(x) => x,
                        Err(e) => return repl_err!(e),
                    };

                    let compensation_millis = match client.public.get_compensation_millis().await {
                        Ok(x) => x,
                        Err(e) => return repl_err!(e),
                    };

                    let slot = match get_current_latest_block_slot(
                        cfg.thread_count,
                        cfg.t0,
                        cfg.genesis_timestamp,
                        compensation_millis,
                    ) {
                        Ok(a) => a.unwrap_or_else(|| Slot::new(0, 0)),
                        Err(e) => return repl_err!(e),
                    };

                    let mut expire_period = slot.period + cfg.operation_validity_periods;
                    if slot.thread >= addr.get_thread(cfg.thread_count) {
                        expire_period += 1;
                    };
                    let sender_public_key = match wallet.find_associated_public_key(addr) {
                        Some(pk) => *pk,
                        None => return repl_err!("Missing public key"),
                    };

                    let content = OperationContent {
                        sender_public_key,
                        fee,
                        expire_period,
                        op,
                    };
                    let op = match wallet.create_operation(content, addr) {
                        Ok(op) => op,
                        Err(e) => return repl_err!(e),
                    };

                    match client.public.send_operations(vec![op]).await {
                        Ok(x) => repl_ok!(format!("Sent operation id : {:?}", x)),
                        Err(e) => repl_err!(e),
                    }
                }
            }
        }
    }
}
