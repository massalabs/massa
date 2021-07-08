use crate::repl::error::ReplError;
use crate::repl::ReplData;
use clap::App;
use clap::Arg;
use communication::network::PeerInfo;
use std::sync::atomic::{AtomicBool, Ordering};
//use crypto::hash::Hash;
use models::block::Block;
use reqwest::blocking::Response;
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

//test for short hash
use bitcoin_hashes;

pub const HASH_SIZE_BYTES: usize = 32;
static FORMAT_SHORT_HASH: AtomicBool = AtomicBool::new(false); //never set to zero.

#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash)]
pub struct Hash(bitcoin_hashes::sha256::Hash);

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            write!(f, "{}", &self.to_bs58_check()[..8])
        } else {
            write!(f, "{}", &self.to_bs58_check())
        }
    }
}

impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            write!(f, "{}", &self.to_bs58_check()[..8])
        } else {
            write!(f, "{}", &self.to_bs58_check())
        }
    }
}

impl Hash {
    pub fn to_bs58_check(&self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }
    pub fn from_bs58_check(data: &str) -> Result<Hash, ReplError> {
        bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| ReplError::HashParseError(format!("{:?}", err)))
            .and_then(|s| Hash::from_bytes(&s))
    }
    pub fn to_bytes(&self) -> [u8; HASH_SIZE_BYTES] {
        use bitcoin_hashes::Hash;
        *self.0.as_inner()
    }
    pub fn from_bytes(data: &[u8]) -> Result<Hash, ReplError> {
        use bitcoin_hashes::Hash;
        use std::convert::TryInto;
        let res_inner: Result<<bitcoin_hashes::sha256::Hash as Hash>::Inner, _> = data.try_into();
        res_inner
            .map(|inner| Hash(bitcoin_hashes::sha256::Hash::from_inner(inner)))
            .map_err(|err| ReplError::HashParseError(format!("{:?}", err)))
    }
}
impl<'de> ::serde::Deserialize<'de> for Hash {
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Hash, D::Error> {
        if d.is_human_readable() {
            struct Base58CheckVisitor;

            impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
                type Value = Hash;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        Hash::from_bs58_check(&v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Hash::from_bs58_check(&v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = Hash;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Hash::from_bytes(v).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

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
                .help("true: display short hash")
                .required(false)
                .takes_value(true),
        );

    // load config
    let config_path = "config/config.toml";
    let cfg = config::Config::from_toml(&read_to_string(config_path).unwrap()).unwrap();

    let mut repl = repl::Repl::new();

    //add commands
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
        .unwrap_or(false);

    if short_hash {
        FORMAT_SHORT_HASH.swap(true, Ordering::Relaxed);
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

fn cmd_staker_info(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/staker_info/{}", data.node_ip, params[0]);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<serde_json::Value>()?;
        println!("staker_info:{:#?}", resp);
    }
    Ok(())
}

fn cmd_network_info(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/network_info", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<serde_json::Value>()?;
        println!("network_info:{:#?}", resp);
    }
    Ok(())
}

fn cmd_stop_node(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let client = reqwest::blocking::Client::new();
    if let Ok(_) = client
        .post(&format!("http://{}/api/v1/stop_node", data.node_ip))
        .send()
    {
        println!("Stoping node");
    }
    Ok(())
}

fn cmd_state(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/state", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<serde_json::Value>()?;
        println!("State:{:#?}", resp);
    }
    Ok(())
}

fn cmd_last_stale(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_stale", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<Vec<(Hash, u64, u8)>>()?;
        println!("Last stale:{:#?}", resp);
    }
    Ok(())
}

fn cmd_last_invalid(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_invalid", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<Vec<(Hash, u64, u8)>>()?;
        println!("Last invalid:{:#?}", resp);
    }
    Ok(())
}

fn cmd_our_ip(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/our_ip", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<String>()?;
        println!("node ip:{:#?}", resp);
    }
    Ok(())
}

fn cmd_peers(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/peers", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<std::collections::HashMap<IpAddr, PeerInfo>>()?;
        println!("peers:{:#?}", resp);
    }
    Ok(())
}

fn cmd_current_parents(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/current_parents", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<Vec<Hash>>()?;
        println!("Parents:{:#?}", resp);
    }
    Ok(())
}

fn cmd_last_final(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/last_final", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<Vec<(Hash, u64, u8)>>()?;
        println!("last finals:{:#?}", resp);
    }
    Ok(())
}

fn cmd_cliques(data: &mut ReplData, _params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/cliques", data.node_ip);
    if let Some(resp) = request_data(data, &url)? {
        let resp = resp.json::<(usize, Vec<Vec<Hash>>)>()?;
        println!("cliques:{:#?}", resp);
    }
    Ok(())
}

fn cmd_get_block(data: &mut ReplData, params: &[&str]) -> Result<(), ReplError> {
    let url = format!("http://{}/api/v1/block/{}", data.node_ip, params[0]);
    if let Some(resp) = request_data(data, &url)? {
        if resp.content_length().unwrap() > 0 {
            let block = resp.json::<Block>()?;
            println!("block: {:#?}", block);
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
            let block = resp.json::<Vec<Hash>>()?;
            println!("blocks: {:#?}", block);
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
            let block = resp.json::<Vec<(Hash, u64, u8, String, Vec<Hash>)>>()?;
            println!("graph: {:#?}", block);
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
