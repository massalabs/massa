// Copyright (c) 2021 MASSA LABS <info@massa.net>

//! Massa node client application.
//!
//! Allow to query a node using the node API.
//! It can be executed as a REPL to run several command in a shell
//! or as CLI using the API command has a parameter.
//!
//! Parameters:
//! * -c (--cli): The format of the displayed command output. Set to false display user-friendly output.
//! * -n (--node): the node IP.
//! * -s (--short) The format of the displayed hash. Set to true display sort hash (default).
//! * -w (--wallet) activate the wallet command, using the file specified.
//!
//! In REPL mode, up and down arrows or tab key can be use to search in the command history.
//!
//! The help command display all available commands.

use crate::data::AddressStates;
use crate::data::GetOperationContent;
use crate::data::WrappedAddressState;
use crate::data::WrappedHash;
use crate::repl::error::ReplError;
use crate::repl::ReplData;
use crate::wallet::Wallet;
use crate::wallet::WalletInfo;
use api::PubkeySig;
use api::{Addresses, OperationIds, PrivateKeys};
use clap::App;
use clap::Arg;
use communication::network::Peer;
use communication::network::Peers;
use consensus::ExportBlockStatus;
use consensus::Status;
use crypto::signature::PrivateKey;
use crypto::{derive_public_key, generate_random_private_key, hash::Hash};
use log::trace;
use models::Address;
use models::Amount;
use models::BlockId;
use models::Operation;
use models::OperationId;
use models::OperationType;
use models::Slot;
use models::StakersCycleProductionStats;
use models::Version;
use reqwest::blocking::Response;
use reqwest::StatusCode;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::str::FromStr;
use std::string::ToString;
use std::sync::atomic::Ordering;
mod client_config;
mod data;
mod repl;
mod wallet;

///Start the massa-client.
fn main() {
    //client has to run mode:
    // * cli mode where a command is provided in client parameters, the cmd is executed and the result return as a json data.
    // * a REPL moode where the command are typed and executed directly inside the client.
    //declare client parameters common for all modes.
    let app = App::new("Massa CLI")
        .version("0.3")
        .author("Massa Labs <info@massa.net>")
        .about("Massa")
        .arg(
            Arg::with_name("cli")
                .short("c")
                .long("cli")
                .value_name("true, false")
                .help("false: set user-friendly output")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("nodeip")
                .short("n")
                .long("node")
                .value_name("IP ADDR")
                .help("IP:PORT of the node, e.g. 127.0.0.1:3030")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("wallet")
                .short("w")
                .long("wallet")
                .value_name("Wallet file path")
                .help("Wallet file to load.")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("shorthash")
                .short("s")
                .long("shorthash")
                .value_name("true, false")
                .help("true: shorten displayed hashes. Doesn't work in command mode")
                .required(false)
                .takes_value(true),
        );

    // load config
    let config_path = "base_config/config.toml";
    let override_config_path = "config/config.toml";
    let mut cfg = config::Config::default();
    cfg.merge(config::File::with_name(config_path))
        .expect("could not load main config file");
    if std::path::Path::new(override_config_path).is_file() {
        cfg.merge(config::File::with_name(override_config_path))
            .expect("could not load override config file");
    }
    let cfg = cfg
        .try_into::<client_config::Config>()
        .expect("error structuring config");

    //add client commands that can be executed.
    // The Repl struct manage command registration for cli mode with clap and REPL mode with rustyline
    //A command can have parameters or not. The number of parameters (min/max) are decalared to detect bad command typing before its execution.
    //Detection is done by clap for cli mode and rustlyline in REPL mode.
    let (mut repl, app) = repl::Repl::new().new_command(
        "set_short_hash",
        "shorten displayed hashes: Parameters: bool: true (short), false(long)",
        1,
        1,
        set_short_hash,
        true,
        app
    )
    .new_command_noargs("our_ip", "get node ip", true, cmd_our_ip)
    .new_command_noargs("peers", "get node peers", true, cmd_peers)
    .new_command_noargs("cliques", "get cliques", true, cmd_cliques)
    .new_command_noargs(
        "current_parents",
        "get current parents",
        true,
        cmd_current_parents,
    )
    .new_command_noargs("last_final", "get latest finals blocks", true, cmd_last_final)
    .new_command(
        "block",
        "get the block with the specified hash. Parameters: block hash",
        1,
        1, //max nb parameters
        true,
        cmd_get_block,
    )
    .new_command(
        "blockinterval",
        "get blocks within the specified time interval. Optional parameters: [from] <start> (included) and [to] <end> (excluded) millisecond timestamp",
    //    &["from", "to"],
        0,
        2,
        true,
        cmd_blockinterval,
    )
    .new_command(
        "graphinterval",
        "get the block graph within the specified time interval. Optional parameters: [from] <start> (included) and [to] <end> (excluded) millisecond timestamp",
        0,
        2, //max nb parameters
        true,
        cmd_graph_interval,
    )
    .new_command_noargs(
        "network_info",
        "network information: own IP address, connected peers",
        true,
        cmd_network_info,
    )
    .new_command_noargs(
        "version",
        "current node version",
        true,
        cmd_version,
    )
    .new_command_noargs("state", "summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count", true, cmd_state)
    .new_command_noargs(
        "last_stale",
        "(hash, thread, slot) for recent stale blocks",
        true,
        cmd_last_stale,
    ).new_command_noargs(
        "staking_addresses",
        "hashset of  staking addresses",
        true,
        cmd_staking_addresses,
    )
    .new_command_noargs(
        "last_invalid",
        "(hash, thread, slot, discard reason) for recent invalid blocks",
        true,
        cmd_last_invalid,
    )
    .new_command(
        "get_operation",
        "returns the operation with the specified id. Parameters: <operation id>",
        1,
        1,
        true,
        cmd_get_operation,
    )
    .new_command_noargs("stop_node", "Gracefully stop the node", true, cmd_stop_node)
    .new_command(
        "unban",
        "unban <ip address>",
        1,
        1, //max nb parameters
        true,
        cmd_unban,
    )
    .new_command(
        "staker_info",
        "staker info from staker address -> (blocks created, next slots in which the address will be selected). Parameter <Address>",
        1,
        1, //max nb parameters
        true,
        cmd_staker_info,
    )
    .new_command(
        "staker_stats",
        "production stats from staker address. Parameters: list of addresses separated by , (no space).",
        1,
        1, //max nb parameters
        true,
        cmd_staker_stats,
    )
    .new_command(
        "register_staking_keys",
        "add a new private key for the node to use to stake: Parameter: <PrivateKey>",
        1,
        1, //max nb parameters
        true,
        cmd_register_staking_keys,
    )
    .new_command(
        "remove_staking_addresses",
        "removes an address used to stake. Parameter : <Address>",
        1,
        1, //max nb parameters
        true,
        cmd_remove_staking_addresses,
    )
    .new_command(
        "next_draws",
        "next draws for given addresses (list of addresses separated by ,  (no space))-> vec (address, slot for which address is selected)",
        1,
        1, //max nb parameters
        true,
        cmd_next_draws,
    )
    .new_command(
        "operations_involving_address",
        "list operations involving the provided address. Note that old operations are forgotten. Parameter: <Address>",
        1,
        1, //max nb parameters
        true,
        cmd_operations_involving_address,
    ).new_command(
        "block_ids_by_creator",
        "list blocks created by the provided address. Note that old blocks are forgotten. Parameter : <Address>",
        1,
        1, //max nb parameters
        true,
        cmd_block_ids_by_creator,
    )
    .new_command(
        "addresses_info",
        "returns the final and candidate balances for a list of addresses. Parameters: list of addresses separated by ,  (no space).",
        1,
        1, //max nb parameters
        true,
        cmd_addresses_info,
    )
    .new_command(
        "cmd_testnet_rewards_program",
        "Returns rewards id. Parameter: <staking_address> <discord_ID> ",
        2,
        2, //max nb parameters
        false,
        cmd_testnet_rewards_program,
    )
    .new_command_noargs(
        "get_active_stakers",
        "returns the active stakers and their roll counts for the current cycle.",
        true,
        cmd_get_active_stakers,
    )
    //non active wallet command
    .new_command_noargs("wallet_info", "Shows wallet info", false, wallet_info)
    .new_command_noargs("wallet_new_privkey", "Generates a new private key and adds it to the wallet. Returns the associated address.", false, wallet_new_privkey)
    .new_command(
        "send_transaction",
        "sends a transaction from <from_address> to <to_address> (from_address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <from_address> <to_address> <amount> <fee>",
        4,
        4, //max nb parameters
        false,
        send_transaction,
    )
    .new_command(
        "wallet_add_privkey",
        "Adds a list of private keys to the wallet. Returns the associated addresses. Parameters: list of private keys separated by ,  (no space).",
        1,
        1, //max nb parameters
        false,
        wallet_add_privkey,
    )
    .new_command(
        "buy_rolls",
        "buy roll count for <address> (address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <address>  <roll count> <fee>",
        3,
        3, //max nb parameters
        false,
        send_buy_roll,
    )
    .new_command(
        "sell_rolls",
        "sell roll count for <address> (address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <address>  <roll count> <fee>",
        3,
        3, //max nb parameters
        false,
        send_sell_roll,
    )


    .split();

    let matches = app.get_matches();

    //cli or not cli output.
    let cli = matches
        .value_of("cli")
        .and_then(|val| {
            FromStr::from_str(val)
                .map_err(|err| {
                    println!("bad cli value, using default");
                    err
                })
                .ok()
        })
        .unwrap_or(false);
    if cli {
        repl.data.cli = true;
    }

    //ip address of the node to connect.
    let node_ip = matches
        .value_of("nodeip")
        .and_then(|node| {
            FromStr::from_str(node)
                .map_err(|err| {
                    println!("bad ip address, using defaults");
                    err
                })
                .ok()
        })
        .unwrap_or(cfg.default_node);
    repl.data.node_ip = node_ip;

    //shorthash is a global parameter that determine the way hash are shown (long (normal) or short).
    let short_hash = matches
        .value_of("shorthash")
        .and_then(|val| {
            FromStr::from_str(val)
                .map_err(|err| {
                    println!("bad short hash value, using default");
                    err
                })
                .ok()
        })
        .unwrap_or(true);

    if !short_hash {
        data::FORMAT_SHORT_HASH.swap(false, Ordering::Relaxed);
    }

    //filename of the wallet file. There's no security around the wallet file.
    let wallet_file_param = matches.value_of("wallet");
    let file_name = match wallet_file_param {
        Some(file_name) => file_name,
        None => "wallet.dat",
    };
    match Wallet::new(file_name) {
        Ok(wallet) => {
            repl.data.wallet = Some(wallet);
            repl.activate_command("wallet_info");
            repl.activate_command("wallet_new_privkey");
            repl.activate_command("send_transaction");
            repl.activate_command("buy_rolls");
            repl.activate_command("sell_rolls");
            repl.activate_command("wallet_add_privkey");
            repl.activate_command("cmd_testnet_rewards_program");
        }
        Err(err) => {
            println!(
                "Error while loading wallet file:{}. No wallet was loaded.",
                err
            );
        }
    }

    match matches.subcommand() {
        (_, None) => {
            repl.run_cmd("help", &[]);
            repl.run();
        }
        (cmd, Some(cmd_args)) => {
            let args: Vec<&str> = cmd_args
                .values_of("")
                .map(|list| list.collect())
                .unwrap_or_default();
            repl.run_cmd(cmd, &args);
        }
    }
}

//General cmd execution
//When user type a command, it's associated to a method bellow.
//Cmd method, get its data from ReplData or provided params (command parameters)
//The cmd is send to the node with a Rest call.
//The node answer is converted to display for REPL using display trait
//Or the return json is printed in cli mode.
//The request_data method manage Node request/answer and cli printing.

fn cmd_get_active_stakers(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/active_stakers", data.node_ip);

    if let Some(resp) = request_data(data, &url)? {
        if resp.status() == StatusCode::OK {
            let rolls = resp.json::<HashMap<Address, u64>>()?;
            println!("Staking addresses (roll count):");
            for (addr, roll_count) in rolls.into_iter() {
                println!("\t{}: {}", addr, roll_count);
            }
        } else {
            println!("not ok status code: {:?}", resp);
        }
    }
    Ok(())
}

fn send_buy_roll(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    if let Some(wallet) = &data.wallet {
        let from_address = Address::from_bs58_check(params[0].trim())
            .map_err(|err| ReplError::AddressCreationError(err.to_string()))?;
        let roll_count: u64 = FromStr::from_str(params[1])
            .map_err(|err| ReplError::GeneralError(format!("Incorrect roll buy count: {}", err)))?;
        let fee = Amount::from_str(params[2])
            .map_err(|err| ReplError::GeneralError(format!("Incorrect fee: {}", err)))?;
        let operation_type = OperationType::RollBuy { roll_count };

        let operation = wallet.create_operation(operation_type, from_address, fee, data)?;
        send_operation(operation, data)?;
    }
    Ok(())
}

fn send_sell_roll(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    if let Some(wallet) = &data.wallet {
        let from_address = Address::from_bs58_check(params[0].trim())
            .map_err(|err| ReplError::AddressCreationError(err.to_string()))?;
        let roll_count: u64 = FromStr::from_str(params[1]).map_err(|err| {
            ReplError::GeneralError(format!("Incorrect roll sell count: {}", err))
        })?;
        let fee = Amount::from_str(params[2])
            .map_err(|err| ReplError::GeneralError(format!("Incorrect fee: {}", err)))?;
        let operation_type = OperationType::RollSell { roll_count };

        let operation = wallet.create_operation(operation_type, from_address, fee, data)?;
        send_operation(operation, data)?;
    }
    Ok(())
}

fn cmd_get_operation(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    //convert specified ops to OperationId
    let op_list = params[0]
        .split(',')
        .map(|str| OperationId::from_bs58_check(str.trim()))
        .collect::<Result<HashSet<OperationId>, _>>();
    let search_op = match op_list {
        Ok(operation_ids) => OperationIds { operation_ids },
        Err(err) => {
            println!(
                "Error during operations conversion, at least one address is invalid: {}",
                err
            );
            return Ok(());
        }
    };

    let url = format!(
        "http://{}/api/v1/get_operations?{}",
        data.node_ip,
        match serde_qs::to_string(&search_op) {
            Ok(s) => s,
            Err(err) => {
                println!(
                    "Error during operations id conversion, could not convert to url: {}",
                    err
                );
                return Ok(());
            }
        }
    );
    if let Some(resp) = request_data(data, &url)? {
        //println!("resp {:?}", resp.text());
        if resp.status() == StatusCode::OK {
            let ops = resp.json::<Vec<(OperationId, data::GetOperationContent)>>()?;
            for (op_id, op) in ops.into_iter() {
                println!("Operation {}:", op_id);
                println!("{}", op);
                println!();
            }
        } else {
            println!("not ok status code: {:?}", resp);
        }
    }

    Ok(())
}

fn wallet_new_privkey(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    if let Some(wallet) = &mut data.wallet {
        let priv_key = generate_random_private_key();
        wallet.add_private_key(priv_key)?;
        let pub_key = derive_public_key(&priv_key);
        let addr = Address::from_public_key(&pub_key).map_err(|err| {
            ReplError::GeneralError(format!(
                "internal error error during address generation:{}",
                err
            ))
        })?;
        if data.cli {
            println!("{}", serde_json::to_string_pretty(&addr)?);
        } else {
            println!("Generated address: {}", addr.to_bs58_check());
        }
    }
    Ok(())
}

fn wallet_add_privkey(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let keys = params[0]
        .split(',')
        .map(|str| PrivateKey::from_bs58_check(str.trim()))
        .collect::<Result<Vec<PrivateKey>, _>>();

    let keys = match keys {
        Ok(keys) => keys,
        Err(err) => {
            println!("Error during keys parsing: {}", err);
            return Ok(());
        }
    };

    for priv_key in keys.into_iter() {
        if let Some(wallet) = &mut data.wallet {
            wallet.add_private_key(priv_key)?;
            let pub_key = derive_public_key(&priv_key);
            let addr = Address::from_public_key(&pub_key).map_err(|err| {
                ReplError::GeneralError(format!(
                    "internal error during address derivation: {}",
                    err
                ))
            })?;
            if data.cli {
                println!("{}", serde_json::to_string_pretty(&addr)?);
            } else {
                println!("Keypair added. Derived address: {}", addr.to_bs58_check());
            }
        }
    }
    Ok(())
}

fn wallet_info(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    if let Some(wallet) = &data.wallet {
        //get wallet addresses balances
        let ordered_addrs = wallet.get_wallet_address_list();
        let display_wallet_info = query_addresses(data, ordered_addrs.iter().cloned().collect())
            .and_then(|resp| {
                if resp.status() != StatusCode::OK {
                    Ok(HashMap::new())
                } else {
                    resp.json::<HashMap<Address, WrappedAddressState>>()
                        .map_err(|err| err.into())
                }
            })
            //            .or_else::<ReplError, _>(|_| Ok("balance not available.".to_string()))
            .or_else::<ReplError, _>(|_| Ok(HashMap::new()))
            .and_then(|balances| {
                if data.cli {
                    serde_json::to_string_pretty(&balances).map_err(|err| err.into())
                } else {
                    let mut s = String::new();
                    writeln!(
                        s,
                        "{}",
                        WalletInfo {
                            wallet: &wallet,
                            balances,
                        }
                    )?;
                    Ok(s)
                }
            })
            .unwrap();
        if data.cli {
            println!(
                "{{\"wallet\":{}, \"balances\":{}}}",
                wallet
                    .to_json_string()
                    .map_err(|err| ReplError::GeneralError(format!(
                        "Internal error during wallet json conversion: {}",
                        err
                    )))?,
                display_wallet_info
            );
        } else {
            print!("wallet.dat: {}", display_wallet_info);
        }
    }

    Ok(())
}

fn send_transaction(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    if let Some(wallet) = &data.wallet {
        let from_address = Address::from_bs58_check(params[0].trim())
            .map_err(|err| ReplError::AddressCreationError(err.to_string()))?;
        let recipient_address = Address::from_bs58_check(params[1])
            .map_err(|err| ReplError::AddressCreationError(err.to_string()))?;
        let amount = Amount::from_str(params[2])
            .map_err(|err| ReplError::GeneralError(format!("Incorrect amount: {}", err)))?;
        let fee = Amount::from_str(params[3])
            .map_err(|err| ReplError::GeneralError(format!("Incorrect fee: {}", err)))?;
        let operation_type = OperationType::Transaction {
            recipient_address,
            amount,
        };

        let operation = wallet.create_operation(operation_type, from_address, fee, data)?;

        send_operation(operation, data)?;
    }

    Ok(())
}

fn set_short_hash(_: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    if bool::from_str(&params[0].to_lowercase())
        .map(|val| data::FORMAT_SHORT_HASH.swap(val, Ordering::Relaxed))
        .is_err()
    {
        println!("Bad parameter:{}, not a boolean (true, false)", params[0]);
    };
    Ok(())
}

fn cmd_addresses_info(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    //convert specified addresses to Address
    let addr_list = params[0]
        .split(',')
        .map(|str| Address::from_bs58_check(str.trim()))
        .collect::<Result<Vec<Address>, _>>();

    let search_addresses = match addr_list {
        Ok(addrs) => addrs,
        Err(err) => {
            println!("Error during addresses parsing: {}", err);
            return Ok(());
        }
    };

    let resp = query_addresses(data, search_addresses.iter().copied().collect())?;
    if resp.status() != StatusCode::OK {
        let status = resp.status();
        let message = resp
            .json::<data::ErrorMessage>()
            .map(|message| message.message)
            .or_else::<ReplError, _>(|err| Ok(format!("{}", err)))
            .unwrap();
        println!("Server error response: {} - {}", status, message);
    } else if data.cli {
        println!("{}", resp.text().unwrap());
    } else {
        let ledger = resp.json::<HashMap<Address, WrappedAddressState>>()?;
        println!(
            "{}",
            AddressStates {
                map: ledger,
                order: search_addresses
            }
        )
    }

    Ok(())
}

fn cmd_staker_info(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/staker_info/{}", data.node_ip, params[0]);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<data::StakerInfo>()?;
        println!("staker_info:");
        println!("{}", resp);
    }
    Ok(())
}

fn cmd_staker_stats(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let addr_list = params[0]
        .split(',')
        .map(|str| Address::from_bs58_check(str.trim()))
        .collect::<Result<HashSet<Address>, _>>();
    let addrs = match addr_list {
        Ok(addrs) => addrs,
        Err(err) => {
            println!("Error during addresses parsing: {}", err);
            return Ok(());
        }
    };
    let url = format!(
        "http://{}/api/v1/staker_stats?{}",
        data.node_ip,
        serde_qs::to_string(&Addresses {
            addrs: addrs.clone()
        })?
    );
    if let Some(resp) = request_data(data, &url)? {
        let mut stakers_prods = resp.json::<Vec<StakersCycleProductionStats>>()?;
        if stakers_prods.is_empty() {
            println!("no available cycle stats");
        }
        stakers_prods.sort_unstable_by_key(|p| p.cycle);
        for cycle_stats in stakers_prods.into_iter() {
            println!(
                "cycle {} ({}) stats:",
                cycle_stats.cycle,
                if cycle_stats.is_final {
                    "final"
                } else {
                    "active"
                }
            );

            for (addr, (n_ok, n_nok)) in cycle_stats.ok_nok_counts.into_iter() {
                if n_ok + n_nok == 0 {
                    println!("\t{}: not selected during cycle", addr);
                } else {
                    println!(
                        "\t{}: {}/{} produced ({}% miss)",
                        addr,
                        n_ok,
                        n_ok + n_nok,
                        n_nok * 100 / (n_ok + n_nok)
                    )
                }
            }
        }
    }
    Ok(())
}

fn cmd_next_draws(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/consensus_config", data.node_ip);
    let resp = reqwest::blocking::get(&url)?;
    if resp.status() != StatusCode::OK {
        return Err(ReplError::GeneralError(format!(
            "Error during node connection. Server response code: {}",
            resp.status()
        )));
    }

    let consensus_cfg = resp.json::<crate::data::ConsensusConfig>()?;

    let addr_list = params[0]
        .split(',')
        .map(|str| Address::from_bs58_check(str.trim()))
        .collect::<Result<HashSet<Address>, _>>();
    let addrs = match addr_list {
        Ok(addrs) => addrs,
        Err(err) => {
            println!("Error during addresses parsing: {}", err);
            return Ok(());
        }
    };
    let url = format!(
        "http://{}/api/v1/next_draws?{}",
        data.node_ip,
        serde_qs::to_string(&Addresses {
            addrs: addrs.clone()
        })?
    );

    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<data::NextDraws>()?;
        let addr_map = resp.content().iter().fold(
            addrs
                .iter()
                .map(|addr| (addr, Vec::new()))
                .collect::<HashMap<_, _>>(),
            |mut map, (addr, slot)| {
                let entry = map.entry(addr).or_insert_with(Vec::new);
                entry.push(slot);
                map
            },
        );
        for (addr, slots) in addr_map {
            println!("Next selected slots of address: {}:", addr);
            for slot in slots.iter() {
                println!(
                    "   Cycle {}, period {}, thread {}",
                    slot.get_cycle(consensus_cfg.periods_per_cycle),
                    slot.period,
                    slot.thread,
                );
            }
            if slots.is_empty() {
                println!("No known selected slots of address: {}", addr);
            }
            println!();
        }
    }
    Ok(())
}

fn cmd_operations_involving_address(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!(
        "http://{}/api/v1/operations_involving_address/{}",
        data.node_ip, params[0]
    );
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<HashMap<WrappedHash, GetOperationContent>>()?;
        println!("operations_involving_address:");
        for (op_id, is_final) in resp {
            println!("operation {} is final: {}", op_id, is_final);
        }
    }
    Ok(())
}

fn cmd_block_ids_by_creator(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!(
        "http://{}/api/v1/block_ids_by_creator/{}",
        data.node_ip, params[0]
    );
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<HashMap<BlockId, Status>>()?;
        println!("block_ids_by_creator:");

        if resp.is_empty() {
            println!("No blocks found.")
        }
        for (block_id, status) in resp {
            println!(
                "block {} status: {}",
                block_id,
                match status {
                    Status::Active => "active",
                    Status::Final => "final",
                }
            );
        }
    }
    Ok(())
}

fn cmd_network_info(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/network_info", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let info = resp.json::<data::NetworkInfo>()?;
        println!("network_info:");
        println!("{}", info);
    }
    Ok(())
}

fn cmd_version(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/version", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let info = resp.json::<Version>()?;
        println!("version:");
        println!("{}", info);
    }
    Ok(())
}

fn cmd_stop_node(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let client = reqwest::blocking::Client::new();
    trace!("before sending request to client in cmd_stop_node in massa-client main");
    client
        .post(&format!("http://{}/api/v1/stop_node", data.node_ip))
        .send()?;
    trace!("after sending request to client in cmd_stop_node in massa-client main");
    println!("Stopping node");
    Ok(())
}

fn cmd_unban(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let ip = match IpAddr::from_str(params[0]) {
        Ok(ip) => ip,
        Err(e) => {
            println!("Error during ip parsing: {}", e);
            return Ok(());
        }
    };
    let client = reqwest::blocking::Client::new();
    client
        .post(&format!("http://{}/api/v1/unban/{}", data.node_ip, ip))
        .send()?;
    println!("Unbanning {}", ip);
    Ok(())
}

fn cmd_register_staking_keys(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let keys_list = params[0]
        .split(',')
        .map(|str| PrivateKey::from_bs58_check(str.trim()))
        .collect::<Result<Vec<PrivateKey>, _>>();

    let keys = match keys_list {
        Ok(keys) => keys,
        Err(err) => {
            println!("Error during keys parsing: {}", err);
            return Ok(());
        }
    };

    let client = reqwest::blocking::Client::new();
    trace!("before sending request to client in cmd_register_staking_keys in massa-client main");
    client
        .post(&format!(
            "http://{}/api/v1/register_staking_keys?{}",
            data.node_ip,
            serde_qs::to_string(&PrivateKeys { keys })?
        ))
        .send()?;
    trace!("after sending request to client in cmd_register_staking_keys in massa-client main");
    println!("Sent register staking keys command to node");
    Ok(())
}

fn cmd_testnet_rewards_program(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let address = Address::from_bs58_check(params[0].trim())
        .map_err(|err| ReplError::AddressCreationError(err.to_string()))?;
    let msg = params[1].as_bytes().to_vec();

    // get address signature
    let addr_sig = if let Some(wallet) = &data.wallet {
        match wallet.sign_message(address, msg.clone()) {
            Some(sig) => sig,
            None => {
                return Err(ReplError::GeneralError(
                    "address not found in wallet".into(),
                ));
            }
        }
    } else {
        return Err(ReplError::GeneralError("wallet unavailable".into()));
    };

    // get node signature
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&format!("http://{}/api/v1/node_sign_message", data.node_ip,))
        .body(msg)
        .send()?;
    let node_sig = if resp.status() == StatusCode::OK {
        resp.json::<PubkeySig>()?
    } else {
        let status = resp.status();
        let message = resp
            .json::<data::ErrorMessage>()
            .map(|message| message.message)
            .or_else::<ReplError, _>(|err| Ok(format!("{}", err)))?;
        return Err(ReplError::GeneralError(format!(
            "Server error response status: {} - {}",
            status, message
        )));
    };

    // print concatenation
    println!(
        "Enter the following in discord: {}/{}/{}/{}",
        node_sig.public_key, node_sig.signature, addr_sig.public_key, addr_sig.signature
    );
    Ok(())
}

fn cmd_remove_staking_addresses(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let addrs_list = params[0]
        .split(',')
        .map(|str| Address::from_bs58_check(str.trim()))
        .collect::<Result<HashSet<Address>, _>>();

    let addrs = match addrs_list {
        Ok(addrs) => addrs,
        Err(err) => {
            println!("Error during keys parsing: {}", err);
            return Ok(());
        }
    };

    let client = reqwest::blocking::Client::new();
    trace!("before sending request to client in cmd_remove_staking_addresses in massa-client main");
    client
        .delete(&format!(
            "http://{}/api/v1/remove_staking_addresses?{}",
            data.node_ip,
            serde_qs::to_string(&Addresses { addrs })?
        ))
        .send()?;
    trace!("after sending request to client in cmd_remove_staking_addresses in massa-client main");
    println!("Sent remove staking keys commande to node");
    Ok(())
}

fn cmd_state(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/state", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<data::State>()?;
        println!("Summary of the current node state");
        println!("{}", resp);
    }
    Ok(())
}

fn cmd_staking_addresses(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/staking_addresses", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<HashSet<Address>>()?;
        println!("Staking Addresses");
        for ad in resp.into_iter() {
            println!("{}\n", ad);
        }
    }
    Ok(())
}

fn cmd_current_parents(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/current_parents", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<(data::WrappedHash, data::WrappedSlot)> =
            data::from_vec_hash_slot(&resp.json::<Vec<(Hash, Slot)>>()?);
        resp.sort_unstable_by_key(|v| (v.1, v.0));
        let formatted = format_node_hash(&mut resp);
        println!("Parents: {:#?}", formatted);
    }
    Ok(())
}

fn cmd_last_stale(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_stale", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<(data::WrappedHash, data::WrappedSlot)> =
            data::from_vec_hash_slot(&resp.json::<Vec<(Hash, Slot)>>()?);
        resp.sort_unstable_by_key(|v| (v.1, v.0));
        let formatted = format_node_hash(&mut resp);
        println!("Last stale: {:#?}", formatted);
    }
    Ok(())
}

fn cmd_last_invalid(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_invalid", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<(data::WrappedHash, data::WrappedSlot)> =
            data::from_vec_hash_slot(&resp.json::<Vec<(Hash, Slot)>>()?);
        resp.sort_unstable_by_key(|v| (v.0, v.1));
        let formatted = format_node_hash(&mut resp);
        println!("Last invalid: {:#?}", formatted);
    }
    Ok(())
}

fn cmd_last_final(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_final", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<(data::WrappedHash, data::WrappedSlot)> =
            data::from_vec_hash_slot(&resp.json::<Vec<(Hash, Slot)>>()?);
        resp.sort_unstable_by_key(|v| (v.1, v.0));
        let formatted = format_node_hash(&mut resp);
        println!("last finals: {:#?}", formatted);
    }
    Ok(())
}

fn cmd_blockinterval(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format_url_with_to_from("blockinterval", data.node_ip, params)?;
    if let Some(resp) = request_data(data, &url)? {
        let mut block: Vec<(data::WrappedHash, data::WrappedSlot)> =
            data::from_vec_hash_slot(&resp.json::<Vec<(Hash, Slot)>>()?);
        if block.is_empty() {
            println!("Block not found.");
        } else {
            block.sort_unstable_by_key(|v| (v.1, v.0));
            let formatted = format_node_hash(&mut block);
            println!("blocks: {:#?}", formatted);
        }
    }

    Ok(())
}

fn cmd_our_ip(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/our_ip", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<Option<IpAddr>>()?;
        match resp {
            Some(ip) => println!("Node IP address: {}", ip),
            None => println!("The node's IP address isn't defined as routable"),
        }
    }
    Ok(())
}

fn cmd_peers(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/peers", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let Peers { peers, .. } = resp.json::<Peers>()?;
        for (
            _,
            Peer {
                peer_info,
                active_nodes,
            },
        ) in peers.into_iter()
        {
            println!(
                "    {}",
                data::WrappedPeerInfo {
                    peer_info,
                    active_nodes
                }
            );
        }
    }
    Ok(())
}

fn cmd_cliques(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/cliques", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let (nb_cliques, clique_list) = resp.json::<(usize, Vec<Vec<(Hash, Slot)>>)>()?;

        println!("Nb of cliques: {}", nb_cliques);
        println!("Cliques: ");
        clique_list
            .into_iter()
            .map(|clique| data::from_vec_hash_slot(&clique))
            .for_each(|mut clique| {
                //use sort_unstable_by to prepare sort by slot
                clique.sort_unstable_by_key(|v| (v.1, v.0));
                let formatted = format_node_hash(&mut clique);
                println!("{:#?}", formatted);
            });
    }
    Ok(())
}

fn cmd_get_block(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/block/{}", data.node_ip, params[0]);
    if let Some(resp) = request_data(data, &url)? {
        if resp.status() == StatusCode::OK {
            let block = resp
                .json::<ExportBlockStatus>()
                .map(data::WrappedBlockStatus::from)?;
            println!("block: {}", block);
        } else {
            println!("block not found.");
        }
    }

    Ok(())
}

fn cmd_graph_interval(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format_url_with_to_from("graph_interval", data.node_ip, params)?;

    if let Some(resp) = request_data(data, &url)? {
        if resp.content_length().unwrap() > 0 {
            let mut block: Vec<(
                data::WrappedHash,
                data::WrappedSlot,
                String,
                Vec<data::WrappedHash>,
            )> = resp
                .json::<Vec<(Hash, Slot, String, Vec<Hash>)>>()?
                .into_iter()
                .map(|(hash1, slot, status, hash2)| {
                    (
                        hash1.into(),
                        slot.into(),
                        status,
                        hash2.iter().map(|h| h.into()).collect(),
                    )
                })
                .collect();

            block.sort_unstable_by_key(|v| (v.1, v.0));
            block.iter().for_each(|(hash, slot, state, parents)| {
                println!("Block: {} Slot: {} Status:{}", hash, slot, state);
                println!("Block parents: {:?}", parents);
                println!();
            });
        } else {
            println!("Empty graph found.");
        }
    }
    Ok(())
}

//utility functions

fn send_operation(operation: Operation, data: &ReplData) -> Result<(), ReplError> {
    let resp = reqwest::blocking::Client::new()
        .post(&format!("http://{}/api/v1/send_operations", data.node_ip))
        .json(&vec![operation])
        .send()?;
    if resp.status() != StatusCode::OK {
        let status = resp.status();
        let message = resp
            .json::<data::ErrorMessage>()
            .map(|message| message.message)
            .or_else::<ReplError, _>(|err| Ok(format!("{}", err)))
            .unwrap();
        println!("Server response error. Status: {} - {}", status, message);
    } else if data.cli {
        println!("{}", resp.text().unwrap());
    } else {
        let opid_list = resp.json::<Vec<OperationId>>()?;
        if opid_list.is_empty() {
            return Err(ReplError::GeneralError(
                "Could not obtain the transaction ID".to_string(),
            ));
        }
        println!("Operation created: {}", opid_list[0]);
    }

    Ok(())
}

fn query_addresses(data: &ReplData, addrs: HashSet<Address>) -> Result<Response, ReplError> {
    let url = format!(
        "http://{}/api/v1/addresses_info?{}",
        data.node_ip,
        serde_qs::to_string(&Addresses { addrs })?
    );
    reqwest::blocking::get(&url).map_err(|err| err.into())
}

fn format_url_with_to_from(
    service: &str,
    node_ip: SocketAddr,
    params: &[&str],
) -> Result<String, ReplError> {
    if let Some(p) = params
        .iter()
        .find(|p| !p.starts_with("from=") && !p.starts_with("to="))
    {
        return Err(ReplError::BadCommandParameter(p.to_string()));
    }
    let from = params
        .iter()
        .filter(|p| p.len() > 5 && p.starts_with("from="))
        .map(|p| p.split_at(5).1)
        .next();
    let to = params
        .iter()
        .filter(|p| p.len() > 3 && p.starts_with("to="))
        .map(|p| p.split_at(3).1)
        .next();
    let url = match (from, to) {
        (None, None) => format!("http://{}/api/v1/{}", node_ip, service),
        (None, Some(to)) => format!("http://{}/api/v1/{}?end={}", node_ip, service, to),
        (Some(from), None) => format!("http://{}/api/v1/{}?start={}", node_ip, service, from),
        (Some(from), Some(to)) => format!(
            "http://{}/api/v1/{}?start={}&end={}",
            node_ip, service, from, to
        ),
    };
    Ok(url)
}

///Send the REST request to the API node.
///
///Return the request response or and Error.
fn request_data(data: &ReplData, url: &str) -> Result<Option<Response>, ReplError> {
    let resp = reqwest::blocking::get(url)?;
    if resp.status() != StatusCode::OK && resp.status() != StatusCode::NOT_FOUND {
        //println!("resp.text(self):{:?}", resp.text());
        let status = resp.status();
        let message = resp
            .json::<data::ErrorMessage>()
            .map(|message| message.message)
            .or_else::<ReplError, _>(|err| Ok(format!("{}", err)))
            .unwrap();
        println!("Server error response status: {} - {}", status, message);
        Ok(None)
    } else if data.cli {
        println!("{}", resp.text()?);
        Ok(None)
    } else {
        Ok(Some(resp))
    }
}

///Construct a list of display String from the specified list of Hash
///The hash are sorted with their slot (period) number
///
///The input parameter list is a collection of tuple (Hash, Slot)
/// return a list of string the display.
fn format_node_hash(list: &mut [(data::WrappedHash, data::WrappedSlot)]) -> Vec<String> {
    list.sort_unstable_by(|a, b| a.1.cmp(&b.1));
    list.iter()
        .map(|(hash, slot)| format!("({} Slot:{})", hash, slot))
        .collect()
}
