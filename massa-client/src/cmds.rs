// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::rpc::Client;
use anyhow::{bail, Result};
use console::style;
use erased_serde::{Serialize, Serializer};
use models::api::{AddressInfo, EndorsementInfo, NodeStatus, OperationInfo};
use models::timeslots::get_current_latest_block_slot;
use models::{
    Address, Amount, BlockId, EndorsementId, OperationContent, OperationId, OperationType, Slot,
};
use signature::{generate_random_private_key, PrivateKey};
use std::fmt::Display;
use std::net::IpAddr;
use std::process;
use strum::{EnumMessage, EnumProperty, IntoEnumIterator};
use strum_macros::{Display, EnumIter, EnumMessage, EnumProperty, EnumString};
use wallet::Wallet;

pub trait Output: Serialize + Display {}
impl dyn Output {
    pub(crate) fn display(&self) {
        println!("{}", self);
    }
    pub(crate) fn json(&self) -> Result<()> {
        let json = &mut serde_json::Serializer::new(std::io::stdout());
        let mut format: Box<dyn Serializer> = Box::new(<dyn Serializer>::erase(json));
        self.erased_serialize(&mut format)?;
        Ok(())
    }
}
impl Output for String {}
impl Output for &str {}
impl Output for NodeStatus {} // TODO: move them close to types definition

macro_rules! display {
    ($expr: expr) => {
        println!("{}", $expr)
    };
}

macro_rules! output {
    ($expr: expr) => {
        Ok(Box::new($expr))
    };
}

// TODO: ugly utilities functions
fn parse_vec<T: std::str::FromStr>(args: &Vec<String>) -> Result<Vec<T>, T::Err> {
    args.iter().map(|x| x.parse::<T>()).collect()
}

// TODO: should be removed to allow JSON outputs
fn format_vec<T: std::fmt::Display>(output: &Vec<T>) -> String {
    output
        .iter()
        .map(|x| format!("{}", x))
        .collect::<Vec<String>>()
        .join("\n")
}

#[allow(non_camel_case_types)]
#[derive(Debug, PartialEq, EnumIter, EnumMessage, EnumString, EnumProperty, Display)]
pub enum Command {
    #[strum(ascii_case_insensitive, message = "exit the client gracefully")]
    exit,

    #[strum(ascii_case_insensitive, message = "display this help")]
    help,

    #[strum(
        ascii_case_insensitive,
        props(args = "[IpAddr]"),
        message = "unban a given IP addresses"
    )]
    unban,

    #[strum(
        ascii_case_insensitive,
        props(args = "[IpAddr]"),
        message = "ban a given IP addresses"
    )]
    ban,

    #[strum(ascii_case_insensitive, message = "stops the node")]
    node_stop,

    #[strum(ascii_case_insensitive, message = "show staking addresses")]
    node_get_staking_addresses,

    #[strum(
        ascii_case_insensitive,
        props(args = "Address1 Address2 ..."),
        message = "remove staking addresses"
    )]
    node_remove_staking_addresses,

    #[strum(
        ascii_case_insensitive,
        props(args = "PrivateKey1 PrivateKey2 ..."),
        message = "add staking private keys"
    )]
    node_add_staking_private_keys,

    #[strum(
        ascii_case_insensitive,
        props(args = "Address discord_id"),
        message = "generate the testnet rewards program node/staker ownership proof"
    )]
    node_testnet_rewards_program_ownership_proof,

    #[strum(
        ascii_case_insensitive,
        message = "show the status of the node (reachable? number of peers connected, consensus, version, config parameter summary...)"
    )]
    get_status,

    #[strum(
        ascii_case_insensitive,
        props(args = "Address1 Address2 ..."),
        message = "get info about a list of addresses (balances, block creation, ...)"
    )]
    get_addresses,

    #[strum(
        ascii_case_insensitive,
        props(args = "BlockId"),
        message = "show info about a block (content, finality ...)"
    )]
    get_block,

    #[strum(
        ascii_case_insensitive,
        props(args = "EndorsementId1 EndorsementId2 ...", todo = ""),
        message = "show info about a list of endorsements (content, finality ...)"
    )]
    get_endorsements,

    #[strum(
        ascii_case_insensitive,
        props(args = "OperationId1 OperationId2 ..."),
        message = "show info about a list of operations(content, finality ...) "
    )]
    get_operations,

    #[strum(
        ascii_case_insensitive,
        message = "show wallet info (private keys, public keys, addresses, balances ...)"
    )]
    wallet_info,

    #[strum(
        ascii_case_insensitive,
        message = "generate a private key and add it into the wallet"
    )]
    wallet_generate_private_key,

    #[strum(
        ascii_case_insensitive,
        props(args = "PrivateKey1 PrivateKey2 ..."),
        message = "add a list of private keys to the wallet"
    )]
    wallet_add_private_keys,

    #[strum(
        ascii_case_insensitive,
        props(args = "Address1 Address2 ..."),
        message = "remove a list of addresses from the wallet"
    )]
    wallet_remove_addresses,

    #[strum(
        ascii_case_insensitive,
        props(args = "Address RollCount Fee"),
        message = "buy rolls with wallet address"
    )]
    buy_rolls,

    #[strum(
        ascii_case_insensitive,
        props(args = "Address RollCount Fee"),
        message = "sell rolls with wallet address"
    )]
    sell_rolls,

    #[strum(
        ascii_case_insensitive,
        props(args = "SenderAddress ReceiverAddress Amount Fee"),
        message = "send coins from a wallet address"
    )]
    send_transaction,
}

pub(crate) fn help() -> String {
    format!(
        "HELP of Massa client (list of available commands):\n{}",
        Command::iter()
            .map(|c| format!("{}", c.help()))
            .collect::<Vec<String>>()
            .join("\n")
    )
}

impl Command {
    pub(crate) fn help(&self) -> String {
        format!(
            "- {} {}: {}{}",
            style(self.to_string()).green(),
            if self.get_str("args").is_some() {
                style(self.get_str("args").unwrap_or("")).yellow()
            } else {
                style("no args").color256(8).italic() // grey
            },
            if self.get_str("todo").is_some() {
                style("[not yet implemented] ").red()
            } else {
                style("")
            },
            self.get_message().unwrap()
        )
    }

    pub(crate) async fn run(
        &self,
        client: &Client,
        wallet: &mut Wallet,
        parameters: &Vec<String>,
    ) -> Result<Box<dyn Output>> {
        match self {
            Command::exit => process::exit(0),

            Command::help => {
                output!(if !parameters.is_empty() {
                    if let Ok(c) = parameters[0].parse::<Command>() {
                        c.help()
                    } else {
                        format!("Command not found!\ntype \"help\" to get the list of commands")
                    }
                } else {
                    help()
                })
            }

            Command::unban => {
                let ips = parse_vec::<IpAddr>(parameters)?;
                match client.private.unban(ips).await {
                    Ok(()) => output!("Request of unbanning successfully sent!"),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::ban => {
                let ips = parse_vec::<IpAddr>(parameters)?;
                match client.private.ban(ips).await {
                    Ok(()) => output!("Request of banning successfully sent!"),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::node_stop => match client.private.stop_node().await {
                Ok(()) => output!("Request of stopping the Node successfully sent"),
                Err(e) => bail!("RpcError: {}", e),
            },

            Command::node_get_staking_addresses => {
                match client.private.get_staking_addresses().await {
                    Ok(staking_addresses) => output!(format!(
                        // TODO
                        "{}",
                        staking_addresses
                            .into_iter()
                            .fold("".to_string(), |acc, a| format!("{}{}\n", acc, a)
                                .to_string())
                    )),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::node_remove_staking_addresses => {
                let addresses = parse_vec::<Address>(parameters)?;
                match client.private.remove_staking_addresses(addresses).await {
                    Ok(()) => output!("Addresses successfully removed!"),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::node_add_staking_private_keys => {
                let private_keys = parse_vec::<PrivateKey>(parameters)?;
                match client.private.add_staking_private_keys(private_keys).await {
                    Ok(()) => output!("Private keys successfully added!"),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::node_testnet_rewards_program_ownership_proof => {
                if parameters.len() != 2 {
                    bail!("Wrong param numbers");
                }
                // parse
                let addr = parameters[0].parse::<Address>()?;
                let msg = parameters[1].as_bytes().to_vec();
                // get address signature
                if let Some(addr_sig) = wallet.sign_message(addr, msg.clone()) {
                    // get node signature
                    match client.private.node_sign_message(msg).await {
                        // print concatenation
                        Ok(node_sig) => {
                            display!("Enter the following in discord:");
                            return output!(format!(
                                "{}/{}/{}/{}", // TODO
                                node_sig.public_key,
                                node_sig.signature,
                                addr_sig.public_key,
                                addr_sig.signature
                            ));
                        }
                        Err(e) => bail!("RpcError: {}", e),
                    }
                } else {
                    bail!("Address not found")
                }
            }

            Command::get_status => match client.public.get_status().await {
                Ok(node_status) => output!(node_status),
                Err(e) => bail!("RpcError: {}", e),
            },

            Command::get_addresses => {
                let addresses = parse_vec::<Address>(parameters)?;
                match client.public.get_addresses(addresses).await {
                    Ok(addresses_info) => output!(format_vec::<AddressInfo>(&addresses_info)),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::get_block => {
                if parameters.len() != 1 {
                    bail!("Wrong param numbers")
                }
                let block_id = parameters[0].parse::<BlockId>()?;
                match client.public.get_block(block_id).await {
                    Ok(block_info) => output!(format!("{}", block_info)),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::get_endorsements => {
                let endorsements = parse_vec::<EndorsementId>(parameters)?;
                match client.public.get_endorsements(endorsements).await {
                    Ok(endorsements_info) => {
                        output!(format_vec::<EndorsementInfo>(&endorsements_info))
                    }
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::get_operations => {
                let operations = parse_vec::<OperationId>(parameters)?;
                match client.public.get_operations(operations).await {
                    Ok(operations_info) => output!(format_vec::<OperationInfo>(&operations_info)),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::wallet_info => {
                let full_wallet = wallet.get_full_wallet();
                // TODO: maybe too much info in wallet_info

                let mut res = "WARNING: do not share your private key\n\n".to_string();

                match client
                    .public
                    .get_addresses(full_wallet.keys().copied().collect())
                    .await
                {
                    Ok(x) => {
                        for info in x.into_iter() {
                            let keys = match full_wallet.get(&info.address) {
                                Some(keys) => keys,
                                None => bail!("Missing keys in wallet"),
                            };
                            res.push_str(&format!(
                                "Private key: {}\nPublic key: {}\n{}\n\n=====\n\n",
                                keys.1,
                                keys.0,
                                info.compact()
                            ));
                        }
                        output!(res)
                    }
                    Err(e) => {
                        res.push_str(&format!(
                            "Error retrieving addresses info: {:?}\nIs your node running ?\n\n",
                            e
                        ));
                        for (ad, (publ, priva)) in full_wallet.into_iter() {
                            res.push_str(&format!(
                                "Private key: {}\nPublic key: {}\nAddress: {}\n\n=====\n\n",
                                priva, publ, ad
                            ));
                        }
                        output!(res)
                    }
                }
            }

            Command::wallet_generate_private_key => {
                let ad = wallet.add_private_key(generate_random_private_key())?;
                output!(format!(
                    "Generated {} address and added it to the wallet",
                    ad
                ))
            }

            Command::wallet_add_private_keys => {
                let mut res = "".to_string();
                for key in parse_vec::<PrivateKey>(parameters)?.into_iter() {
                    let ad = wallet.add_private_key(key)?;
                    res.push_str(&format!("Derived and added address {} to the wallet\n", ad));
                }
                output!(res)
            }

            Command::wallet_remove_addresses => {
                let mut res = "".to_string();
                for key in parse_vec::<Address>(parameters)?.into_iter() {
                    match wallet.remove_address(key) {
                        Some(_) => {
                            res.push_str(&format!("Removed address {} from the wallet\n", key));
                        }
                        None => {
                            res.push_str(&format!("Address {} wasn't in the wallet\n", key));
                        }
                    }
                }
                output!(res)
            }

            Command::buy_rolls => {
                if parameters.len() != 3 {
                    bail!("Wrong param numbers"); // TODO: print help buy roll
                }
                let addr = parameters[0].parse::<Address>()?;
                let roll_count = parameters[1].parse::<u64>()?;
                let fee = parameters[2].parse::<Amount>()?;

                send_operation(
                    client,
                    wallet,
                    OperationType::RollBuy { roll_count },
                    fee,
                    addr,
                )
                .await
            }

            Command::sell_rolls => {
                if parameters.len() != 3 {
                    bail!("Wrong param numbers"); // TODO: print help sell roll
                }
                let addr = parameters[0].parse::<Address>()?;
                let roll_count = parameters[1].parse::<u64>()?;
                let fee = parameters[2].parse::<Amount>()?;

                send_operation(
                    client,
                    wallet,
                    OperationType::RollSell { roll_count },
                    fee,
                    addr,
                )
                .await
            }

            Command::send_transaction => {
                if parameters.len() != 4 {
                    bail!("Wrong param numbers"); // TODO: print help transaction
                }
                let addr = parameters[0].parse::<Address>()?;
                let recipient_address = parameters[1].parse::<Address>()?;
                let amount = parameters[2].parse::<Amount>()?;
                let fee = parameters[3].parse::<Amount>()?;

                send_operation(
                    client,
                    wallet,
                    OperationType::Transaction {
                        recipient_address,
                        amount,
                    },
                    fee,
                    addr,
                )
                .await
            }
        }
    }
}

async fn send_operation(
    client: &Client,
    wallet: &Wallet,
    op: OperationType,
    fee: Amount,
    addr: Address,
) -> Result<Box<dyn Output>> {
    let cfg = match client.public.get_status().await {
        Ok(node_status) => node_status,
        Err(e) => bail!("RpcError: {}", e),
    }
    .algo_config;

    let slot = get_current_latest_block_slot(cfg.thread_count, cfg.t0, cfg.genesis_timestamp, 0)?
        .unwrap_or_else(|| Slot::new(0, 0));
    let mut expire_period = slot.period + cfg.operation_validity_periods;
    if slot.thread >= addr.get_thread(cfg.thread_count) {
        expire_period += 1;
    };
    let sender_public_key = match wallet.find_associated_public_key(addr) {
        Some(pk) => *pk,
        None => bail!("Missing public key"),
    };

    let op = wallet.create_operation(
        OperationContent {
            sender_public_key,
            fee,
            expire_period,
            op,
        },
        addr,
    )?;

    match client.public.send_operations(vec![op]).await {
        Ok(operation_ids) => {
            display!("Sent operation IDs:");
            output!(format_vec(&operation_ids))
        }
        Err(e) => bail!("RpcError: {}", e),
    }
}
