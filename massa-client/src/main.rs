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

use crate::repl::error::ReplError;
use crate::repl::ReplData;
use clap::App;
use clap::Arg;
use crypto::{
    hash::Hash,
    signature::{derive_public_key, PrivateKey},
};
use log::trace;
use models::Address;
use models::Operation;
use models::OperationId;
use models::SerializeCompact;
use models::{Block, Slot};
use models::{OperationContent, OperationType};
use reqwest::blocking::Response;
use reqwest::StatusCode;
use std::string::ToString;

use crate::wallet::Wallet;
use communication::network::PeerInfo;
use consensus::LedgerDataExport;
use std::fs::read_to_string;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::Ordering;

mod config;
mod data;
mod repl;
mod wallet;

///Start the massa-client.
fn main() {
    let app = App::new("Massa CLI")
        .version("1.0")
        .author("Massa Labs <contact@massa.network>")
        .about("Massa")
        .arg(
            Arg::with_name("nodeip")
                .short("n")
                .long("node")
                .value_name("IP ADDR")
                .help("Ip:Port of the node, ex: 127.0.0.1:3030")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("wallet")
                .short("w")
                .long("wallet")
                .value_name("Wallet file path")
                .help("Activate wallet command.")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("shorthash")
                .short("s")
                .long("shorthash")
                .value_name("true, false")
                .help("true: display short hash. Doesn't work in command mode")
                .required(false)
                .takes_value(true),
        );

    // load config
    let config_path = "config/config.toml";
    let cfg = config::Config::from_toml(&read_to_string(config_path).unwrap()).unwrap();

    //add commands
    let (mut repl, app) = repl::Repl::new().new_command(
        "set_short_hash",
        "set displayed hash short: Parameter: bool: true (short), false(long)",
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
    .new_command_noargs("last_final", "get last finals blocks", true, cmd_last_final)
    .new_command(
        "block",
        "get the block with the specifed hash. Parameter: block hash",
        1,
        1, //max nb parameters
        true,
        cmd_get_block,
    )
    .new_command(
        "blockinterval",
        "get the block within the specifed time interval. Optinal parameters: [from] <start> and [to] <end> (excluded) time interval. ",
    //    &["from", "to"],
        0,
        2,
        true,
        cmd_blockinterval,
    )
    .new_command(
        "graphinterval",
        "get the block graph within the specifed time interval. Optinal parameters: [from] <start> and [to] <end> (excluded) time interval",
        0,
        2, //max nb parameters
        true,
        cmd_graph_interval,
    )
    .new_command_noargs(
        "network_info",
        "network information: own IP address, connected peers (IP)",
        true,
        cmd_network_info,
    )
    .new_command_noargs("state", "summary of the current state: time, last final block (hash, thread, slot, timestamp), nb cliques, nb connected nodes", true, cmd_state)
    .new_command_noargs(
        "last_stale",
        "(hash, thread, slot) for last stale blocks",
        true,
        cmd_last_stale,
    )
    .new_command_noargs(
        "last_invalid",
        "(hash, thread, slot, reason) for last invalid blocks",
        true,
        cmd_last_invalid,
    )
    .new_command(
        "create_transaction",
        "create a new transaction with specified parameters. parameters: <private_key> <recipient address> <amount> <fee> <expire_period>",
        5,
        5,
        true,
        cmd_create_transaction,
    )
    .new_command_noargs("stop_node", "Stop node gracefully", true, cmd_stop_node)
    .new_command(
        "staker_info",
        "staker info from staker address (pubkey hash) -> (blocks created, next slots where address is selected)",
        1,
        1, //max nb parameters
        true,
        cmd_staker_info,
    )
    .new_command(
        "address_info",
        "return the specified address balance for current final block and best parents parameters: <Address Hash>",
        1,
        1, //max nb parameters
        true,
        cmd_address_info,
    )
    //non active wellet command
    .new_command_noargs("wallet_info", "Show wallet info.", false, wallet_info)
    .new_command_noargs("wallet_new_privkey", "Generates a new private key and adds it to the wallet file. Return the associated public key address.", false, wallet_new_privkey)
    .new_command(
        "send_transaction",
        "send a transaction <from_address> to <to_address>(if that address has a private key in the wallet or else error). Return the OperationId parameters: <from_address> <to_address> <amount> <fee>",
        4,
        4, //max nb parameters
        false,
        send_transaction,
    )


    .split();

    let matches = app.get_matches();

    let node_ip = matches
        .value_of("nodeip")
        .and_then(|node| {
            FromStr::from_str(node)
                .map_err(|err| {
                    println!("bad ip address, use default one");
                    err
                })
                .ok()
        })
        .unwrap_or(cfg.default_node.clone());
    repl.data.node_ip = node_ip;

    let short_hash = matches
        .value_of("shorthash")
        .and_then(|val| {
            FromStr::from_str(val)
                .map_err(|err| {
                    println!("bad short hash value, use default one");
                    err
                })
                .ok()
        })
        .unwrap_or(true);

    if !short_hash {
        data::FORMAT_SHORT_HASH.swap(false, Ordering::Relaxed);
    }

    let wallet_file_param = matches.value_of("wallet");
    if let Some(file_name) = wallet_file_param {
        println!("open wallet");
        match Wallet::new(file_name) {
            Ok(wallet) => {
                repl.data.wallet = Some(wallet);
                repl.activate_command("wallet_info");
                repl.activate_command("wallet_new_privkey");
                repl.activate_command("send_transaction");
            }
            Err(err) => {
                println!(
                    "Error during loading wallet file:{}. No wallet is actif.",
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
                .or(Some(vec![]))
                .unwrap();
            repl.data.cli = true;
            repl.run_cmd(cmd, &args);
        }
    }
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
            println!("Generated private key address:{}", addr.to_bs58_check());
        }
    }
    Ok(())
}

fn wallet_info(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    if let Some(wallet) = &data.wallet {
        if data.cli {
            println!(
                "{}",
                wallet
                    .to_json_string()
                    .map_err(|err| ReplError::GeneralError(format!(
                        "internal error during wallet json conversion:{}",
                        err
                    )))?
            );
        } else {
            println!("{}", wallet);
        }
    }

    Ok(())
}

fn send_transaction(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    if let Some(wallet) = &data.wallet {
        //get node serialisation context
        let url = format!("http://{}/api/v1/node_config", data.node_ip);
        let resp = reqwest::blocking::get(&url)?;
        if resp.status() != StatusCode::OK {
            return Err(ReplError::GeneralError(format!(
                "Error during node connection. Server answer code :{}",
                resp.status()
            )));
        }
        let context = resp.json::<models::SerializationContext>()?;
        //get pool config
        let url = format!("http://{}/api/v1/pool_config", data.node_ip);
        let resp = reqwest::blocking::get(&url)?;
        if resp.status() != StatusCode::OK {
            return Err(ReplError::GeneralError(format!(
                "Error during node connection. Server answer code :{}",
                resp.status()
            )));
        }
        let pool_cfg = resp.json::<pool::PoolConfig>()?;
        //get consensus config
        let url = format!("http://{}/api/v1/consensus_config", data.node_ip);
        let resp = reqwest::blocking::get(&url)?;
        if resp.status() != StatusCode::OK {
            return Err(ReplError::GeneralError(format!(
                "Error during node connection. Server answer code :{}",
                resp.status()
            )));
        }
        let consensus_cfg = resp.json::<crate::data::ConsensusConfig>()?;

        //get from address private key
        let from_address = Address::from_bs58_check(params[0].trim())
            .map_err(|err| ReplError::AddressCreationError(err.to_string()))?;
        let private_key =
            wallet
                .find_associated_private_key(from_address)
                .ok_or(ReplError::GeneralError(format!(
                    "No private key found in the wallet for specified from address:{}",
                    params[0].trim()
                )))?;
        let public_key = derive_public_key(&private_key);

        let recipient_address = Address::from_bs58_check(params[1])
            .map_err(|err| ReplError::AddressCreationError(err.to_string()))?;
        let amount: u64 = FromStr::from_str(&params[2]).map_err(|err| {
            ReplError::GeneralError(format!(
                "Error incorrect specified amount not an int :{}",
                err
            ))
        })?;
        let fee: u64 = FromStr::from_str(&params[3]).map_err(|err| {
            ReplError::GeneralError(format!("Error incorrect specified fee not an int :{}", err))
        })?;

        let slot = consensus::get_current_latest_block_slot(
            consensus_cfg.thread_count,
            consensus_cfg.t0,
            consensus_cfg.genesis_timestamp,
            0,
        )
        .map_err(|err| {
            ReplError::GeneralError(format!(
                "Error during current time slot generation :{}",
                err
            ))
        })?
        .ok_or(ReplError::GeneralError(
            "Error no current time slot generated".to_string(),
        ))?;

        let expire_period = slot.period + pool_cfg.max_operation_future_validity_start_periods;
        let operation_type = OperationType::Transaction {
            recipient_address,
            amount,
        };
        let operation_content = OperationContent {
            fee,
            expire_period,
            sender_public_key: public_key,
            op: operation_type,
        };

        let hash = Hash::hash(&operation_content.to_bytes_compact(&context).unwrap());
        let signature = crypto::sign(&hash, &private_key).unwrap();

        let operation = Operation {
            content: operation_content,
            signature,
        };
        let resp = reqwest::blocking::Client::new()
            .post(&format!("http://{}/api/v1/operations", data.node_ip))
            .json(&vec![operation])
            .send()?;
        if resp.status() != StatusCode::OK {
            let status = resp.status();
            let message = resp
                .json::<data::ErrorMessage>()
                .map(|message| message.message)
                .or_else::<ReplError, _>(|err| Ok(format!("{}", err)))
                .unwrap();
            println!("The serveur answer status:{} an error:{}", status, message);
        } else {
            let opid_list = resp.json::<Vec<OperationId>>()?;
            if opid_list.len() == 0 {
                return Err(ReplError::GeneralError(
                    "Error no operation id generated during transaction send".to_string(),
                ));
            }
            if data.cli {
                println!("{}", opid_list[0]);
            } else {
                println!("Operation created:{}", opid_list[0]);
            }
        }
    }

    Ok(())
}

//create_transaction <private_key> <recipient address> <amount> <fee> <expire_period>
fn cmd_create_transaction(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    //get node serialisation context
    let url = format!("http://{}/api/v1/node_config", data.node_ip);
    let resp = reqwest::blocking::get(&url)?;
    if resp.status() != StatusCode::OK {
        return Err(ReplError::GeneralError(format!(
            "Error during node connection. Server answer code :{}",
            resp.status()
        )));
    }

    let context = resp.json::<models::SerializationContext>()?;
    //create a dummy transaction
    let private_key = PrivateKey::from_bs58_check(params[0].trim())
        .map_err(|_| ReplError::UnreconnizedKeyError)?;

    let public_key = derive_public_key(&private_key);
    let recipient_address = Address::from_bs58_check(params[1])
        .map_err(|err| ReplError::AddressCreationError(err.to_string()))?;
    let amount: u64 = FromStr::from_str(&params[2]).map_err(|err| {
        ReplError::GeneralError(format!("incorrect specified amount not an int :{}", err))
    })?;
    let fee: u64 = FromStr::from_str(&params[3]).map_err(|err| {
        ReplError::GeneralError(format!("incorrect specified fee not an int :{}", err))
    })?;
    let expire_period: u64 = FromStr::from_str(&params[4]).map_err(|err| {
        ReplError::GeneralError(format!(
            "incorrect specified expire period not an int :{}",
            err
        ))
    })?;
    let operation_type = OperationType::Transaction {
        recipient_address,
        amount,
    };
    let operation_content = OperationContent {
        fee,
        expire_period,
        sender_public_key: public_key,
        op: operation_type,
    };

    let hash = Hash::hash(&operation_content.to_bytes_compact(&context).unwrap());
    let signature = crypto::sign(&hash, &private_key).unwrap();

    let operation = Operation {
        content: operation_content,
        signature,
    };
    let resp = reqwest::blocking::Client::new()
        .post(&format!("http://{}/api/v1/operations", data.node_ip))
        .json(&vec![operation])
        .send()?;
    if resp.status() != StatusCode::OK {
        let status = resp.status();
        let message = resp
            .json::<data::ErrorMessage>()
            .map(|message| message.message)
            .or_else::<ReplError, _>(|err| Ok(format!("{}", err)))
            .unwrap();
        println!("The serveur answer status:{} an error:{}", status, message);
    } else {
        println!("Transaction created");
    }
    trace!("after sending request to client in cmd_create_transaction in massa-client main");

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

fn cmd_address_info(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    //convert specified address to Address
    let search_address = match Address::from_bs58_check(params[0]) {
        Ok(addr) => addr,
        Err(err) => {
            println!(
                "Error during address convertion, provided address not a bs58 Hash :{}",
                err
            );
            return Ok(());
        }
    };

    let url = format!(
        "http://{}/api/v1/address_data/{}",
        data.node_ip, search_address
    );
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp
            .json::<LedgerDataExport>()
            .map(|ledger| data::WrapperAddressLedgerDataExport::new(&search_address, ledger))?;
        println!("ledger info for address:{}", search_address);
        println!("{}", resp);
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
        println!("Summary of current node state");
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
        println!("Parents:{:#?}", formated);
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
        println!("Last stale:{:#?}", formated);
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
        println!("Last invalid:{:#?}", formated);
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
        println!("last finals:{:#?}", formated);
    }
    Ok(())
}

fn cmd_blockinterval(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format_url_with_to_from("blockinterval", data.node_ip, params)?;
    if let Some(resp) = request_data(data, &url)? {
        let mut block: Vec<(data::WrappedHash, data::WrappedSlot)> =
            data::from_vec_hash_slot(&resp.json::<Vec<(Hash, Slot)>>()?);
        if block.len() == 0 {
            println!("Not block found.");
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
            Some(ip) => println!("Our IP address: {}", ip),
            None => println!("Our IP address isn't defined"),
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
        if resp.content_length().unwrap() > 0 {
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
    if resp.status() != StatusCode::OK {
        //println!("resp.text(self):{:?}", resp.text());
        let status = resp.status();
        let message = resp
            .json::<data::ErrorMessage>()
            .map(|message| message.message)
            .or_else::<ReplError, _>(|err| Ok(format!("{}", err)))
            .unwrap();
        println!("The serveur answer status:{} an error:{}", status, message);
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
