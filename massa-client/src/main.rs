use crate::repl::error::ReplError;
use crate::repl::ReplData;
use clap::App;
use clap::Arg;
use reqwest::blocking::Response;
use std::fs::read_to_string;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::atomic::Ordering;

/*
Usage:
 Connection node is defined by default in the file config/config.toml
 use -n or --node to override the default node.

 Use tab, up arrow to complet or search in command history.
 Command history is saved in the file config/history.txt.
 Use the command help to see all available command

*/

mod config;
mod data;
mod repl;

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

    let mut repl = repl::Repl::new();

    //add commands
    let app = repl.new_command(
        "set_short_hash",
        "set displayed hash short: Parameter: bool: true (short), false(long)",
        1,
        1,
        set_short_hash,
        app,
    );
    let app = repl.new_command_noargs("our_ip", "get node ip", cmd_our_ip, app);
    let app = repl.new_command_noargs("peers", "get node peers", cmd_peers, app);
    let app = repl.new_command_noargs("cliques", "get cliques", cmd_cliques, app);
    let app = repl.new_command_noargs(
        "current_parents",
        "get current parents",
        cmd_current_parents,
        app,
    );
    let app = repl.new_command_noargs("last_final", "get last finals blocks", cmd_last_final, app);
    let app = repl.new_command(
        "block",
        "get the block with the specifed hash. Parameter: block hash",
        1,
        1, //max nb parameters
        cmd_get_block,
        app,
    );
    let app = repl.new_command(
        "blockinterval",
        "get the block within the specifed time interval. Parameters: start and end time interval",
        2,
        2, //max nb parameters
        cmd_blockinterval,
        app,
    );
    let app = repl.new_command(
        "graphinterval",
        "get the block graph within the specifed time interval. Parameters: start and end time interval",
        2,
        2, //max nb parameters
        cmd_graph_interval, app,
    );

    let app = repl.new_command_noargs(
        "network_info",
        "network information: own IP address, connected peers (IP)",
        cmd_network_info,
        app,
    );
    let app = repl.new_command_noargs("state", "summary of the current state: time, last final block (hash, thread, slot, timestamp), nb cliques, nb connected nodes", cmd_state, app);
    let app = repl.new_command_noargs(
        "last_stale",
        "(hash, thread, slot) for last stale blocks",
        cmd_last_stale,
        app,
    );
    let app = repl.new_command_noargs(
        "last_invalid",
        "(hash, thread, slot, reason) for last invalid blocks",
        cmd_last_invalid,
        app,
    );

    let app = repl.new_command_noargs("stop_node", "Stop node gracefully", cmd_stop_node, app);

    let app = repl.new_command(
        "staker_info",
        "staker info from staker address (pubkey hash) -> (blocks created, next slots where address is selected)",
        1,
        1, //max nb parameters
        cmd_staker_info, app,
    );

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
        .map(|val| data::FORMAT_SHORT_HASH.swap(val, Ordering::Relaxed))
    {
        println!("Bad parameter:{}, not a boolean (true, false)", params[0]);
    };
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
    client
        .post(&format!("http://{}/api/v1/stop_node", data.node_ip))
        .send()?;
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

fn cmd_last_stale(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_stale", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp = resp.json::<Vec<(data::Hash, u64, u8)>>()?;
        resp.sort_unstable_by(|a, b| data::compare_hash(&(a.0, (a.1, a.2)), &(b.0, (b.1, b.2))));
        let formated = format_node_hash(&mut resp);
        println!("Last stale:{:?}", formated);
    }
    Ok(())
}

fn cmd_last_invalid(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_invalid", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp = resp.json::<Vec<(data::Hash, u64, u8)>>()?;
        resp.sort_unstable_by(|a, b| data::compare_hash(&(a.0, (a.1, a.2)), &(b.0, (b.1, b.2))));
        let formated = format_node_hash(&mut resp);
        println!("Last invalid:{:?}", formated);
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
        let resp = resp.json::<std::collections::HashMap<IpAddr, data::PeerInfo>>()?;
        for peer in resp.values() {
            println!("    {}", peer);
        }
    }
    Ok(())
}

fn cmd_current_parents(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/current_parents", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp = resp.json::<Vec<(data::Hash, (u64, u8))>>()?;
        resp.sort_unstable_by(|a, b| data::compare_hash(a, b));
        println!("Parents:{:?}", resp);
    }
    Ok(())
}

fn cmd_last_final(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_final", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let mut resp = resp.json::<Vec<(data::Hash, u64, u8)>>()?;
        resp.sort_unstable_by(|a, b| data::compare_hash(&(a.0, (a.1, a.2)), &(b.0, (b.1, b.2))));
        let formated = format_node_hash(&mut resp);
        println!("last finals:{:?}", formated);
    }
    Ok(())
}

fn cmd_cliques(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/cliques", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let (nb_cliques, clique_list) =
            resp.json::<(usize, Vec<Vec<(data::Hash, (u64, u8))>>)>()?;
        println!("Nb of cliques: {}", nb_cliques);
        println!("Cliques: ");
        clique_list.into_iter().for_each(|mut clique| {
            //use sort_unstable_by to prepare sort by slot
            clique.sort_unstable_by(|a, b| data::compare_hash(a, b));
            println!("{:?}", clique);
        });
    }
    Ok(())
}

fn cmd_get_block(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/block/{}", data.node_ip, params[0]);
    if let Some(resp) = request_data(data, &url)? {
        if resp.content_length().unwrap() > 0 {
            let block = resp.json::<data::Block>()?;
            println!("block: {}", block);
        } else {
            println!("block not found.");
        }
    }

    Ok(())
}

fn cmd_blockinterval(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!(
        "http://{}/api/v1/blockinterval/{}/{}",
        data.node_ip, params[0], params[1]
    );
    if let Some(resp) = request_data(data, &url)? {
        if resp.content_length().unwrap() > 0 {
            let mut block = resp.json::<Vec<(data::Hash, (u64, u8))>>()?;
            block.sort_unstable_by(|a, b| data::compare_hash(a, b));
            println!("blocks: {:?}", block);
        } else {
            println!("Not block found.");
        }
    }

    Ok(())
}

fn cmd_graph_interval(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!(
        "http://{}/api/v1/graph_interval/{}/{}",
        data.node_ip, params[0], params[1]
    );
    if let Some(resp) = request_data(data, &url)? {
        if resp.content_length().unwrap() > 0 {
            let mut block = resp.json::<Vec<(data::Hash, u64, u8, String, Vec<data::Hash>)>>()?;
            block.sort_unstable_by(|a, b| {
                data::compare_hash(&(a.0, (a.1, a.2)), &(b.0, (b.1, b.2)))
            });
            block
                .iter()
                .for_each(|(hash, slot, thread, state, parents)| {
                    println!(
                        "Block: {} Period: {} Thread:{} Status:{}",
                        hash, slot, thread, state
                    );
                    println!("Block parents: {:?}", parents);
                    println!("");
                });
        } else {
            println!("Empty graph found.");
        }
    }

    Ok(())
}

fn request_data(data: &ReplData, url: &str) -> Result<Option<Response>, ReplError> {
    let resp = reqwest::blocking::get(url)?;
    if data.cli {
        println!("{}", resp.text()?);
        Ok(None)
    } else {
        Ok(Some(resp))
    }
}

fn format_node_hash(list: &mut [(data::Hash, u64, u8)]) -> Vec<String> {
    list.sort_unstable_by(|a, b| a.1.cmp(&b.1));
    list.iter()
        .map(|(hash, slot, thread)| format!("({} Period:{} th:{})", hash, slot, thread))
        .collect()
}
