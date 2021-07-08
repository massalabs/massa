//! Massa node client application.
//!
//! Allow to query a node using the node API.
//! It can be executed as a REPL to run several command in a shell
//! or as CLI using the API command has a parameter.
//!
//! Parameters:
//! * -n (--node): the node IP
//! * -s (--short) The format of the displayed hash. Set to true display sort hash (default).
//! * -w (--wallet) activate the wallet command, using the file specified.
//!
//! In REPL mode, up and down arrows or tab key can be use to search in the command history.
//!
//! The help command display all available commands.

use crate::data::GetOperationContent;
use crate::data::WrappedHash;
use crate::repl::error::ReplError;
use crate::repl::ReplData;
use crate::wallet::Wallet;
use api::{Addresses, OperationIds};
use clap::App;
use clap::Arg;
use communication::network::PeerInfo;
use consensus::LedgerDataExport;
use crypto::{hash::Hash, signature::derive_public_key};
use log::trace;
use models::Address;
use models::AddressRollState;
use models::Operation;
use models::OperationId;
use models::{Block, Slot};
use models::{OperationContent, OperationType};
use reqwest::blocking::Response;
use reqwest::StatusCode;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write;
use std::fs::read_to_string;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::str::FromStr;
use std::string::ToString;
use std::sync::atomic::Ordering;
mod config;
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
    let config_path = "config/config.toml";
    let cfg = config::Config::from_toml(&read_to_string(config_path).unwrap()).unwrap();

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
    .new_command_noargs("last_final", "get lates finals blocks", true, cmd_last_final)
    .new_command(
        "block",
        "get the block with the specifed hash. Parameters: block hash",
        1,
        1, //max nb parameters
        true,
        cmd_get_block,
    )
    .new_command(
        "blockinterval",
        "get blocks within the specifed time interval. Optional parameters: [from] <start> (included) and [to] <end> (excluded) millisecond timestamp",
    //    &["from", "to"],
        0,
        2,
        true,
        cmd_blockinterval,
    )
    .new_command(
        "graphinterval",
        "get the block graph within the specifed time interval. Optional parameters: [from] <start> (included) and [to] <end> (excluded) millisecond timestamp",
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
    .new_command_noargs("state", "summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count", true, cmd_state)
    .new_command_noargs(
        "last_stale",
        "(hash, thread, slot) for recent stale blocks",
        true,
        cmd_last_stale,
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
        "staker_info",
        "staker info from staker address -> (blocks created, next slots in which the address will be selected)",
        1,
        1, //max nb parameters
        true,
        cmd_staker_info,
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
        "list operations involving the provided address. Note that old operations are forgotten.",
        1,
        1, //max nb parameters
        true,
        cmd_operations_involving_address,
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
        "addresses_roll_state",
        "returns the final and candidate roll state for a list of addresses. Parameters: list of addresses separated by ,  (no space).",
        1,
        1, //max nb parameters
        true,
        cmd_address_roll_state,
    )
    //non active wellet command
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
        .unwrap_or(cfg.default_node.clone());
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
    if let Some(file_name) = wallet_file_param {
        match Wallet::new(file_name) {
            Ok(wallet) => {
                repl.data.wallet = Some(wallet);
                repl.activate_command("wallet_info");
                repl.activate_command("wallet_new_privkey");
                repl.activate_command("send_transaction");
            }
            Err(err) => {
                println!(
                    "Error while loading wallet file:{}. No wallet was loaded.",
                    err
                );
            }
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
                .unwrap_or(vec![]);
            repl.data.cli = true;
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

fn cmd_address_roll_state(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
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
        "http://{}/api/v1/addresses_roll_state?{}",
        data.node_ip,
        serde_qs::to_string(&Addresses { addrs })?
    );

    if let Some(resp) = request_data(data, &url)? {
        //println!("resp {:?}", resp.text());
        if resp.status() == StatusCode::OK {
            let rolls = resp.json::<Vec<(Address, AddressRollState)>>()?;
            for (addr, roll_state) in rolls.into_iter() {
                println!("Roll for addresses:");
                println!(
                    "({}: final rolls: {}, active_rolls: {}, candidate_rolls: {}):",
                    addr,
                    roll_state.final_rolls,
                    if let Some(nb) = roll_state.active_rolls {
                        nb.to_string()
                    } else {
                        "none".to_string()
                    },
                    roll_state.candidate_rolls,
                );
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
        let roll_count: u64 = FromStr::from_str(&params[2]).map_err(|err| {
            ReplError::GeneralError(format!("Incorrect transaction amount: {}", err))
        })?;
        let fee: u64 = FromStr::from_str(&params[3])
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
        let roll_count: u64 = FromStr::from_str(&params[2]).map_err(|err| {
            ReplError::GeneralError(format!("Incorrect transaction amount: {}", err))
        })?;
        let fee: u64 = FromStr::from_str(&params[3])
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
                println!("");
            }
        } else {
            println!("not ok status code: {:?}", resp);
        }
    }

    Ok(())
}

fn wallet_new_privkey(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    if let Some(wallet) = &mut data.wallet {
        let priv_key = crypto::generate_random_private_key();
        wallet.add_private_key(priv_key)?;
        let pub_key = crypto::derive_public_key(&priv_key);
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

fn wallet_info(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    if let Some(wallet) = &data.wallet {
        //get wallet addresses balances
        let balances = query_addresses(&data, wallet.get_wallet_address_list())
            .and_then(|resp| {
                if resp.status() != StatusCode::OK {
                    Ok("balance not available.".to_string())
                } else {
                    if data.cli {
                        Ok(format!("{}", resp.text().unwrap()))
                    } else {
                        let ledger = resp.json::<LedgerDataExport>()?;
                        let balance_list = data::extract_addresses_from_ledger(&ledger);
                        if balance_list.len() == 0 {
                            Ok("Error no balance found".to_string())
                        } else {
                            let mut s = String::new();
                            for b in balance_list {
                                writeln!(s, "{}", b)?;
                            }
                            Ok(s)
                        }
                    }
                }
            })
            .or::<ReplError>(Ok("balance not available.".to_string()))
            .unwrap();
        if data.cli {
            println!(
                "{{wallet:{}, balance:{}}}",
                wallet
                    .to_json_string()
                    .map_err(|err| ReplError::GeneralError(format!(
                        "Internal error during wallet json conversion: {}",
                        err
                    )))?,
                balances
            );
        } else {
            println!("wallet:{}", wallet);
            println!("balance:");
            println!("{}", balances);
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
        let amount: u64 = FromStr::from_str(&params[2]).map_err(|err| {
            ReplError::GeneralError(format!("Incorrect transaction amount: {}", err))
        })?;
        let fee: u64 = FromStr::from_str(&params[3])
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
    if let Err(_) = bool::from_str(&params[0].to_lowercase())
        .map(|val| data::FORMAT_SHORT_HASH.swap(val, Ordering::Relaxed))
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
        .collect::<Result<HashSet<Address>, _>>();

    let search_addresses = match addr_list {
        Ok(addrs) => addrs,
        Err(err) => {
            println!("Error during addresses parsing: {}", err);
            return Ok(());
        }
    };

    let resp = query_addresses(&data, search_addresses)?;
    if resp.status() != StatusCode::OK {
        let status = resp.status();
        let message = resp
            .json::<data::ErrorMessage>()
            .map(|message| message.message)
            .or_else::<ReplError, _>(|err| Ok(format!("{}", err)))
            .unwrap();
        println!("Server error response: {} - {}", status, message);
    } else {
        if data.cli {
            println!("{}", resp.text().unwrap());
        } else {
            let ledger = resp.json::<LedgerDataExport>()?;
            let balance_list = data::extract_addresses_from_ledger(&ledger);
            if balance_list.len() == 0 {
                return Err(ReplError::GeneralError("No balance found.".to_string()));
            } else {
                for export in balance_list {
                    println!("{}", export);
                }
            }
        }
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

fn cmd_next_draws(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/next_draws/{}", data.node_ip, params[0]);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<data::NextDraws>()?;
        println!("next_draws:");
        println!("{}", resp);
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

fn cmd_network_info(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/network_info", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let info = resp.json::<data::NetworkInfo>()?;
        println!("network_info:");
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
    println!("Stoping node");
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

fn cmd_current_parents(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/current_parents", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<(data::WrappedHash, data::WrappedSlot)> =
            data::from_vec_hash_slot(&resp.json::<Vec<(Hash, Slot)>>()?);
        resp.sort_unstable_by_key(|v| (v.1, v.0));
        let formated = format_node_hash(&mut resp);
        println!("Parents: {:#?}", formated);
    }
    Ok(())
}

fn cmd_last_stale(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_stale", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<(data::WrappedHash, data::WrappedSlot)> =
            data::from_vec_hash_slot(&resp.json::<Vec<(Hash, Slot)>>()?);
        resp.sort_unstable_by_key(|v| (v.1, v.0));
        let formated = format_node_hash(&mut resp);
        println!("Last stale: {:#?}", formated);
    }
    Ok(())
}

fn cmd_last_invalid(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_invalid", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<(data::WrappedHash, data::WrappedSlot)> =
            data::from_vec_hash_slot(&resp.json::<Vec<(Hash, Slot)>>()?);
        resp.sort_unstable_by_key(|v| (v.0, v.1));
        let formated = format_node_hash(&mut resp);
        println!("Last invalid: {:#?}", formated);
    }
    Ok(())
}

fn cmd_last_final(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_final", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<(data::WrappedHash, data::WrappedSlot)> =
            data::from_vec_hash_slot(&resp.json::<Vec<(Hash, Slot)>>()?);
        resp.sort_unstable_by_key(|v| (v.1, v.0));
        let formated = format_node_hash(&mut resp);
        println!("last finals: {:#?}", formated);
    }
    Ok(())
}

fn cmd_blockinterval(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format_url_with_to_from("blockinterval", data.node_ip, params)?;
    if let Some(resp) = request_data(data, &url)? {
        let mut block: Vec<(data::WrappedHash, data::WrappedSlot)> =
            data::from_vec_hash_slot(&resp.json::<Vec<(Hash, Slot)>>()?);
        if block.len() == 0 {
            println!("Block not found.");
        } else {
            block.sort_unstable_by_key(|v| (v.1, v.0));
            let formated = format_node_hash(&mut block);
            println!("blocks: {:#?}", formated);
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
        let resp = resp.json::<std::collections::HashMap<IpAddr, PeerInfo>>()?;
        for peer in resp.values() {
            println!("    {}", data::WrappedPeerInfo::from(peer));
        }
    }
    Ok(())
}

fn cmd_cliques(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/cliques", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let (nb_cliques, clique_list) = resp.json::<(usize, Vec<Vec<(Hash, Slot)>>)>()?;
        let wrapped_clique_list: Vec<Vec<(data::WrappedHash, data::WrappedSlot)>> = clique_list
            .into_iter()
            .map(|clique| data::from_vec_hash_slot(&clique))
            .collect();

        println!("Nb of cliques: {}", nb_cliques);
        println!("Cliques: ");
        wrapped_clique_list.into_iter().for_each(|mut clique| {
            //use sort_unstable_by to prepare sort by slot
            clique.sort_unstable_by_key(|v| (v.1, v.0));
            let formated = format_node_hash(&mut clique);
            println!("{:#?}", formated);
        });
    }
    Ok(())
}

fn cmd_get_block(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/block/{}", data.node_ip, params[0]);
    if let Some(resp) = request_data(data, &url)? {
        if resp.status() == StatusCode::OK {
            let block = resp
                .json::<Block>()
                .map(|block| data::WrapperBlock::from(block))?;
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
                println!("");
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
    } else {
        if data.cli {
            println!("{}", resp.text().unwrap());
        } else {
            let opid_list = resp.json::<Vec<OperationId>>()?;
            if opid_list.len() == 0 {
                return Err(ReplError::GeneralError(
                    "Could not obtain the transaction ID".to_string(),
                ));
            }
            println!("Operation created: {}", opid_list[0]);
        }
    }

    Ok(())
}

fn query_addresses<'a>(data: &'a ReplData, addrs: HashSet<Address>) -> Result<Response, ReplError> {
    let url = format!(
        "http://{}/api/v1/addresses_data?{}",
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
        .filter(|p| !p.starts_with("from=") && !p.starts_with("to="))
        .next()
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
///Return the request reponse or and Error.
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
    } else {
        if data.cli {
            println!("{}", resp.text()?);
            Ok(None)
        } else {
            Ok(Some(resp))
        }
    }
}

///Construct a list of diplay String from the specified list of Hash
///The hash are sorted with their slot (periode) number
///
///The input parameter list is a collection of tuple (Hash, Slot)
/// return a list of string the display.
fn format_node_hash(list: &mut [(data::WrappedHash, data::WrappedSlot)]) -> Vec<String> {
    list.sort_unstable_by(|a, b| a.1.cmp(&b.1));
    list.iter()
        .map(|(hash, slot)| format!("({} Slot:{})", hash, slot))
        .collect()
}
