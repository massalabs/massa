use crate::repl::error::ReplError;
use crate::repl::ReplData;
use clap::App;
use clap::Arg;
use crypto::hash::Hash;
use models::block::Block;
use std::fs::read_to_string;
use std::net::IpAddr;
use std::str::FromStr;

/*
Usage:
 Connection node is defined by default in the file config/config.toml
 use -n or --node to override the default node.

 Use tab, up arrow to complet or search in command history.
 Command history is saved in the file config/history.txt.
 Use the command help to see all available command

*/

mod config;
mod repl;

fn main() -> Result<(), ReplError> {
    let matches = App::new("Massa CLI")
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
        .get_matches();

    // load config
    let config_path = "config/config.toml";
    let cfg = config::Config::from_toml(&read_to_string(config_path).unwrap()).unwrap();

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

    let mut repl = repl::Repl::new();
    repl.data.node_ip = node_ip;

    //add commands
    repl.new_command_noargs("our_ip", "get node ip", cmd_our_ip);
    repl.new_command_noargs("peers", "get node peers", cmd_peers);
    repl.new_command_noargs("cliques", "get cliques", cmd_cliques);
    repl.new_command_noargs(
        "current_parents",
        "get current parents",
        cmd_current_parents,
    );
    repl.new_command_noargs("last_final", "get last finals blocks", cmd_last_final);
    repl.new_command(
        "block",
        "get the block with the specifed hash. Parameter: block hash",
        1, //max nb parameters
        cmd_get_block,
    );
    repl.new_command(
        "blockinterval",
        "get the block within the specifed time interval. Parameters: start and end time interval",
        2, //max nb parameters
        cmd_blockinterval,
    );
    repl.new_command(
        "graphinterval",
        "get the block graph within the specifed time interval. Parameters: start and end time interval",
        2, //max nb parameters
        cmd_graph_interval,
    );

    repl.new_command_noargs(
        "network_info",
        "network information: own IP address, connected peers (IP)",
        cmd_network_info,
    );
    repl.new_command_noargs("state", "summary of the current state: time, last final block (hash, thread, slot, timestamp), nb cliques, nb connected nodes", cmd_state);
    repl.new_command_noargs(
        "last_stale",
        "(hash, thread, slot) for last stale blocks",
        cmd_last_stale,
    );
    repl.new_command_noargs(
        "last_invalid",
        "(hash, thread, slot, reason) for last invalid blocks",
        cmd_last_invalid,
    );

    repl.new_command(
        "staker_info",
        "staker info from staker address (pubkey hash) -> (blocks created, next slots where address is selected)",
        1, //max nb parameters
        cmd_staker_info,
    );

    repl.run()?;
    Ok(())
}

fn cmd_staker_info(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/staker_info/{}", data.node_ip, params[0]);
    let resp = reqwest::blocking::get(&url)?.json::<serde_json::Value>()?;
    println!("staker_info:{:#?}", resp);
    Ok(())
}

fn cmd_network_info(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/network_info", data.node_ip);
    let resp = reqwest::blocking::get(&url)?.json::<serde_json::Value>()?;
    println!("network_info:{:#?}", resp);
    Ok(())
}

fn cmd_state(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/state", data.node_ip);
    let resp = reqwest::blocking::get(&url)?.json::<serde_json::Value>()?;
    println!("State:{:#?}", resp);
    Ok(())
}

fn cmd_last_stale(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_stale", data.node_ip);
    let resp = reqwest::blocking::get(&url)?.json::<Vec<(Hash, u64, u8)>>()?;
    println!("Last stale:{:#?}", resp);
    Ok(())
}

fn cmd_last_invalid(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_invalid", data.node_ip);
    let resp = reqwest::blocking::get(&url)?.json::<Vec<(Hash, u64, u8)>>()?;
    println!("Last invalid:{:#?}", resp);
    Ok(())
}

fn cmd_our_ip(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/our_ip", data.node_ip);
    let resp = reqwest::blocking::get(&url)?.json::<String>()?;
    println!("node ip:{:#?}", resp);
    Ok(())
}

fn cmd_peers(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/peers", data.node_ip);
    let resp = reqwest::blocking::get(&url)?.json::<std::collections::HashMap<IpAddr, String>>()?;
    println!("peers:{:#?}", resp);
    Ok(())
}

fn cmd_current_parents(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/current_parents", data.node_ip);
    let resp = reqwest::blocking::get(&url)?.json::<Vec<Hash>>()?;
    println!("Parents:{:#?}", resp);
    Ok(())
}

fn cmd_last_final(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_final", data.node_ip);
    let resp = reqwest::blocking::get(&url)?.json::<Vec<(Hash, u64, u8)>>()?;
    println!("last finals:{:#?}", resp);
    Ok(())
}

fn cmd_cliques(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/cliques", data.node_ip);
    let resp = reqwest::blocking::get(&url)?.json::<(usize, Vec<Vec<Hash>>)>()?;
    println!("cliques:{:#?}", resp);
    Ok(())
}

fn cmd_get_block(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/block/{}", data.node_ip, params[0]);
    let resp = reqwest::blocking::get(&url)?;
    if resp.content_length().unwrap() > 0 {
        let block = resp.json::<Block>()?;
        println!("block: {:#?}", block);
    } else {
        println!("block not found.");
    }

    Ok(())
}

fn cmd_blockinterval(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!(
        "http://{}/api/v1/blockinterval/{}/{}",
        data.node_ip, params[0], params[1]
    );
    let resp = reqwest::blocking::get(&url)?;
    if resp.content_length().unwrap() > 0 {
        let block = resp.json::<Vec<Hash>>()?;
        println!("blocks: {:#?}", block);
    } else {
        println!("Not block found.");
    }

    Ok(())
}

fn cmd_graph_interval(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!(
        "http://{}/api/v1/graph_interval/{}/{}",
        data.node_ip, params[0], params[1]
    );
    let resp = reqwest::blocking::get(&url)?;
    if resp.content_length().unwrap() > 0 {
        let block = resp.json::<Vec<(Hash, u64, u8, String, Vec<Hash>)>>()?;
        println!("graph: {:#?}", block);
    } else {
        println!("Empty graph found.");
    }

    Ok(())
}
