//! Massa node client application.
//!
//! Allow to query a node using the node API.
//! It can be executed as a REPL to run several command in a shell
//! or as CLI using the API command has a parameter.
//!
//! Parameters:
//! * -n (--node): the node IP
//! * -s (--short) The format of the displayed hash. Set to true display sort hash (default).
//!
//! In REPL mode, up and down arrows or tab key can be use to search in the command history.
//!
//! The help command display all available commands.

use crate::repl::error::ReplError;
use crate::repl::ReplData;
use clap::App;
use clap::Arg;
use communication::network::PeerInfo;
use models::Block;
use reqwest::blocking::Response;
use reqwest::StatusCode;
use serde::Deserialize;
use std::fs::read_to_string;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::Ordering;

mod config;
//mod data;
mod repl;

pub const HASH_SIZE_BYTES: usize = 32;

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
        app
    )
    .new_command_noargs("our_ip", "get node ip", cmd_our_ip)
    .new_command_noargs("peers", "get node peers", cmd_peers)
    .new_command_noargs("cliques", "get cliques", cmd_cliques)
    .new_command_noargs(
        "current_parents",
        "get current parents",
        cmd_current_parents,
    )
    .new_command_noargs("last_final", "get last finals blocks", cmd_last_final)
    .new_command(
        "block",
        "get the block with the specifed hash. Parameter: block hash",
        1,
        1, //max nb parameters
        cmd_get_block,
    )
    .new_command(
        "blockinterval",
        "get the block within the specifed time interval. Optinal parameters: [from] <start> and [to] <end> (excluded) time interval. ",
    //    &["from", "to"],
        0,
        2,
        cmd_blockinterval,
    )
    .new_command(
        "graphinterval",
        "get the block graph within the specifed time interval. Optinal parameters: [from] <start> and [to] <end> (excluded) time interval",
        0,
        2, //max nb parameters
        cmd_graph_interval,
    )
    .new_command_noargs(
        "network_info",
        "network information: own IP address, connected peers (IP)",
        cmd_network_info,
    )
    .new_command_noargs("state", "summary of the current state: time, last final block (hash, thread, slot, timestamp), nb cliques, nb connected nodes", cmd_state)
    .new_command_noargs(
        "last_stale",
        "(hash, thread, slot) for last stale blocks",
        cmd_last_stale,
    )
    .new_command_noargs(
        "last_invalid",
        "(hash, thread, slot, reason) for last invalid blocks",
        cmd_last_invalid,
    )
    .new_command_noargs("stop_node", "Stop node gracefully", cmd_stop_node)
    .new_command(
        "staker_info",
        "staker info from staker address (pubkey hash) -> (blocks created, next slots where address is selected)",
        1,
        1, //max nb parameters
        cmd_staker_info,
    ).split();

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
        apimodel::FORMAT_SHORT_HASH.swap(false, Ordering::Relaxed);
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

fn set_short_hash(_: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    if let Err(_) = bool::from_str(&params[0].to_lowercase())
        .map(|val| apimodel::FORMAT_SHORT_HASH.swap(val, Ordering::Relaxed))
    {
        println!("Bad parameter:{}, not a boolean (true, false)", params[0]);
    };
    Ok(())
}

fn cmd_staker_info(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/staker_info/{}", data.node_ip, params[0]);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<apimodel::StakerInfo>()?;
        println!("staker_info:");
        println!("{}", resp);
    }
    Ok(())
}

fn cmd_network_info(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/network_info", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let info = resp.json::<apimodel::NetworkInfo>()?;
        println!("network_info:");
        println!("{}", info);
    }
    Ok(())
}

fn cmd_stop_node(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let client = reqwest::blocking::Client::new();
    client
        .post(&format!("http://{}/api/v1/stop_node", data.node_ip))
        .send()?;
    println!("Stoping node");
    Ok(())
}

fn cmd_state(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/state", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<apimodel::State>()?;
        println!("Summary of current node state");
        println!("{}", resp);
    }
    Ok(())
}

fn cmd_current_parents(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/current_parents", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<apimodel::HashSlot> = resp.json()?;
        resp.sort_unstable_by_key(|v| (v.slot, v.hash));
        let formated: Vec<String> = resp.iter().map(|hash_slot| hash_slot.to_string()).collect();
        println!("Parents:{:#?}", formated);
    }
    Ok(())
}

fn cmd_last_stale(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_stale", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<apimodel::HashSlot> = resp.json()?;
        resp.sort_unstable_by_key(|v| (v.slot, v.hash));
        let formated: Vec<String> = resp.iter().map(|hash_slot| hash_slot.to_string()).collect();
        println!("Last stale:{:#?}", formated);
    }
    Ok(())
}

fn cmd_last_invalid(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_invalid", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<apimodel::HashSlot> = resp.json()?;
        resp.sort_unstable_by_key(|v| (v.slot, v.hash));
        let formated: Vec<String> = resp.iter().map(|hash_slot| hash_slot.to_string()).collect();
        println!("Last invalid:{:#?}", formated);
    }
    Ok(())
}

fn cmd_last_final(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_final", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp: Vec<apimodel::HashSlot> = resp.json()?;
        resp.sort_unstable_by_key(|v| (v.slot, v.hash));
        let formated: Vec<String> = resp.iter().map(|hash_slot| hash_slot.to_string()).collect();
        println!("last finals:{:#?}", formated);
    }
    Ok(())
}

fn cmd_blockinterval(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format_url_with_to_from("blockinterval", data.node_ip, params)?;
    if let Some(resp) = request_data(data, &url)? {
        let mut blocks: Vec<apimodel::HashSlot> = resp.json()?;
        if blocks.len() == 0 {
            println!("Not block found.");
        } else {
            blocks.sort_unstable_by_key(|v| (v.slot, v.hash));
            let formated: Vec<String> = blocks
                .iter()
                .map(|hash_slot| hash_slot.to_string())
                .collect();
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
        let resp = resp.json::<Vec<PeerInfo>>()?;
        for peer in resp {
            println!("    {}", apimodel::WrappedPeerInfo::from(peer));
        }
    }
    Ok(())
}

fn cmd_cliques(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/cliques", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let cliques = resp.json::<apimodel::Cliques>()?;
        println!("{}", cliques);
    }
    Ok(())
}

fn cmd_get_block(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/block/{}", data.node_ip, params[0]);
    if let Some(resp) = request_data(data, &url)? {
        if resp.content_length().unwrap() > 0 {
            let block = resp
                .json::<Block>()
                .map(|block| apimodel::WrapperBlock::from(block))?;
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
            let mut blocks = resp.json::<Vec<apimodel::BlockInfo>>()?;
            blocks.sort_unstable_by_key(|v| (v.hash_slot.slot, v.hash_slot.hash));
            blocks.iter().for_each(|block| {
                println!("{}", block);
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
        let message = resp
            .json::<ErrorMessage>()
            .map(|message| message.message)
            .or_else::<ReplError, _>(|err| Ok(format!("{}", err)))
            .unwrap();
        println!("The serveur answer an error:{}", message);
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

#[derive(Clone, Debug, Deserialize)]
pub struct ErrorMessage {
    pub message: String,
}
