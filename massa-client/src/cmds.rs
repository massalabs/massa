// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::repl::Output;
use crate::rpc::Client;
use anyhow::{bail, Result};
use console::style;
use models::timeslots::get_current_latest_block_slot;
use models::{
    Address, Amount, BlockId, EndorsementId, OperationContent, OperationId, OperationType, Slot,
};
use signature::{generate_random_private_key, PrivateKey};
use std::fmt::Debug;
use std::net::IpAddr;
use std::process;
use strum::{EnumMessage, EnumProperty, IntoEnumIterator};
use strum_macros::{Display, EnumIter, EnumMessage, EnumProperty, EnumString};
use wallet::Wallet;

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

pub(crate) fn help() {
    println!("HELP of Massa client (list of available commands):");
    Command::iter().map(|c| c.help()).collect()
}

impl Command {
    pub(crate) fn help(&self) {
        println!(
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
        json: bool,
    ) -> Result<Box<dyn Output>> {
        match self {
            Command::exit => process::exit(0),

            Command::help => {
                if !json {
                    if !parameters.is_empty() {
                        if let Ok(c) = parameters[0].parse::<Command>() {
                            c.help();
                        } else {
                            println!(
                                "Command not found!\ntype \"help\" to get the list of commands"
                            );
                            help();
                        }
                    } else {
                        help();
                    }
                }
                Ok(Box::new(()))
            }

            Command::unban => {
                let ips = parse_vec::<IpAddr>(parameters)?;
                match client.private.unban(ips).await {
                    Ok(()) => {
                        if !json {
                            println!("Request of unbanning successfully sent!")
                        }
                    }
                    Err(e) => bail!("RpcError: {}", e),
                };
                Ok(Box::new(()))
            }

            Command::ban => {
                let ips = parse_vec::<IpAddr>(parameters)?;
                match client.private.ban(ips).await {
                    Ok(()) => {
                        if !json {
                            println!("Request of banning successfully sent!")
                        }
                    }
                    Err(e) => bail!("RpcError: {}", e),
                }
                Ok(Box::new(()))
            }

            Command::node_stop => {
                match client.private.stop_node().await {
                    Ok(()) => {
                        if !json {
                            println!("Request of stopping the Node successfully sent")
                        }
                    }
                    Err(e) => bail!("RpcError: {}", e),
                };
                Ok(Box::new(()))
            }

            Command::node_get_staking_addresses => {
                match client.private.get_staking_addresses().await {
                    Ok(staking_addresses) => Ok(Box::new(staking_addresses)),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::node_remove_staking_addresses => {
                let addresses = parse_vec::<Address>(parameters)?;
                match client.private.remove_staking_addresses(addresses).await {
                    Ok(()) => {
                        if !json {
                            println!("Addresses successfully removed!")
                        }
                    }
                    Err(e) => bail!("RpcError: {}", e),
                }
                Ok(Box::new(()))
            }

            Command::node_add_staking_private_keys => {
                let private_keys = parse_vec::<PrivateKey>(parameters)?;
                match client.private.add_staking_private_keys(private_keys).await {
                    Ok(()) => {
                        if !json {
                            println!("Private keys successfully added!")
                        }
                    }
                    Err(e) => bail!("RpcError: {}", e),
                };
                Ok(Box::new(()))
            }

            Command::node_testnet_rewards_program_ownership_proof => {
                if parameters.len() != 2 {
                    bail!("wrong param numbers");
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
                            if !json {
                                println!("Enter the following in discord:");
                                println!(
                                    "{}/{}/{}/{}",
                                    node_sig.public_key,
                                    node_sig.signature,
                                    addr_sig.public_key,
                                    addr_sig.signature
                                );
                            }
                        }
                        Err(e) => bail!("RpcError: {}", e),
                    }
                } else {
                    panic!("address not found")
                }
                Ok(Box::new(()))
            }

            Command::get_status => match client.public.get_status().await {
                Ok(node_status) => Ok(Box::new(node_status)),
                Err(e) => bail!("RpcError: {}", e),
            },

            Command::get_addresses => {
                let addresses = parse_vec::<Address>(parameters)?;
                match client.public.get_addresses(addresses).await {
                    Ok(addresses_info) => Ok(Box::new(addresses_info)),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::get_block => {
                if parameters.len() != 1 {
                    bail!("wrong param numbers")
                }
                let block_id = parameters[0].parse::<BlockId>()?;
                match client.public.get_block(block_id).await {
                    Ok(block_info) => Ok(Box::new(block_info)),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::get_endorsements => {
                let endorsements = parse_vec::<EndorsementId>(parameters)?;
                match client.public.get_endorsements(endorsements).await {
                    Ok(endorsements_info) => Ok(Box::new(endorsements_info)),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::get_operations => {
                let operations = parse_vec::<OperationId>(parameters)?;
                match client.public.get_operations(operations).await {
                    Ok(operations_info) => Ok(Box::new(operations_info)),
                    Err(e) => bail!("RpcError: {}", e),
                }
            }

            Command::wallet_info => {
                let full_wallet = wallet.get_full_wallet();
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
                    }
                }
                if !json {
                    println!("{}", res);
                }
                Ok(Box::new(()))
            }

            Command::wallet_generate_private_key => {
                let ad = wallet.add_private_key(generate_random_private_key())?;
                if !json {
                    println!("Generated {} address and added it to the wallet", ad);
                }
                Ok(Box::new(()))
            }

            Command::wallet_add_private_keys => {
                let mut res = "".to_string();
                for key in parse_vec::<PrivateKey>(parameters)?.into_iter() {
                    let ad = wallet.add_private_key(key)?;
                    res.push_str(&format!("Derived and added address {} to the wallet\n", ad));
                }
                if !json {
                    println!("{}", res);
                }
                Ok(Box::new(()))
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
                if !json {
                    println!("{}", res);
                }
                Ok(Box::new(()))
            }

            Command::buy_rolls => {
                if parameters.len() != 3 {
                    bail!("wrong param numbers");
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
                    json,
                )
                .await
            }

            Command::sell_rolls => {
                if parameters.len() != 3 {
                    bail!("wrong param numbers");
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
                    json,
                )
                .await
            }

            Command::send_transaction => {
                if parameters.len() != 4 {
                    bail!("wrong param numbers");
                }
                let addr = parameters[0].parse::<Address>()?;
                let recipient_address = parameters[1].parse::<Address>()?;
                let amount = parameters[2].parse::<Amount>()?;
                let fee = parameters[3].parse::<Amount>()?;

                match amount.checked_add(fee) {
                    Some(total) => {
                        if let Ok(addresses_info) = client.public.get_addresses(vec![addr]).await {
                            if !addresses_info.is_empty()
                                && addresses_info[0].ledger_info.candidate_ledger_info.balance
                                    < total
                            {
                                println!("Warning: this operation may be rejected due to insuffisant balance");
                            }
                        }
                    }
                    None => {
                        bail!("The total amount hit the limit overflow, operation rejected")
                    }
                }

                send_operation(
                    client,
                    wallet,
                    OperationType::Transaction {
                        recipient_address,
                        amount,
                    },
                    fee,
                    addr,
                    json,
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
    json: bool,
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
            if !json {
                println!("Sent operation IDs:");
            }
            Ok(Box::new(operation_ids))
        }
        Err(e) => bail!("RpcError: {}", e),
    }
}

// TODO: ugly utilities functions
pub fn parse_vec<T: std::str::FromStr>(args: &Vec<String>) -> anyhow::Result<Vec<T>, T::Err> {
    args.iter().map(|x| x.parse::<T>()).collect()
}
