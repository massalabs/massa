//! The goal of this API is to retrieve information
//! on the current state of our node and interact with it.
//! In version 0.1, we can get some informations
//! and stop the node through the API.
//!
//! # Examples
//! ## Get cliques
//! Returns a vec containing all cliques, as sets of hashes.
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path("/api/v1/cliques")
//!     .reply(&filter)
//!     .await;
//! ```
//!
//! ## Current parents
//! Returns a vec containing current best parents.
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path("/api/v1/current_parents")
//!     .reply(&filter)
//!     .await;
//! ```
//!
//! ## Block interval
//! Returns all blocks generate between start and end, in millis.
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path(&format!(
//!         "/api/v1/blockinterval?start={}&end={}",
//!         start, end
//!     ))
//!     .reply(&filter)
//!     .await;
//! ```
//!
//! ## Last final
//! Returns last final block in each thread.
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path("/api/v1/last_final")
//!     .reply(&filter)
//!     .await;
//! ```
//!
//! ## Peers
//! Returns all known peers with full peer info.
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path("/api/v1/peers")
//!     .reply(&filter)
//!     .await;
//! ```
//!
//! ## Graph interval
//! Returns some information on blocks generated between start and end, in millis :
//! - hash
//! - period_number,
//! - header.thread_number,
//! - status in ["final", "active", "stale"],
//! - parents,
//!
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path(&format!(
//!         "/api/v1/graph_interval?start={}&end={}",
//!         start, end
//!     ))
//!     .reply(&filter)
//!     .await;
//! ```
//!
//! ## Get block
//! Returns full block associated with given hash.
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path(&format!("/api/v1/block/{}", some_hash))
//!     .reply(&filter)
//!     .await;
//! ```
//!
//! ## Network info
//! Returns our ip and known peers with full peer info.
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path("/api/v1/network_info")
//!     .reply(&filter)
//!     .await;
//! ```
//!
//! ## State
//! Returns current node state :
//! - last final blocks
//! - number of active cliques
//! - number of peers
//! - our ip
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path("/api/v1/state")
//!     .reply(&filter)
//!     .await;
//! ```
//!
//! ## Last stale
//! Returns a number (see configuration) of recent stale blocks.
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path("/api/v1/last_stale")
//!     .reply(&filter)
//!     .await;
//! ```
//!
//! ## Last invalid
//! Returns a number (see configuration) of recent invalid blocks.
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path("/api/v1/last_invalid")
//!     .reply(&filter)
//!     .await;
//! ```
//!
//! ### Staker info
//! Returns information about staker using given public key :
//! - staker active blocks: active blocks created by staker
//! - staker discarded blocks: discarded block created by staker
//! - staker next draw: next slot when staker can create a block
//!
//!
//! ```ignore
//! let res = warp::test::request()
//!     .method("GET")
//!     .path(&format!("/api/v1/staker_info/{}", staker))
//!     .reply(&filter)
//!     .await;
//! ```

use super::config::ApiConfig;
use communication::{
    network::{NetworkCommandSender, NetworkConfig},
    protocol::{ProtocolCommandSender, ProtocolConfig},
};
use consensus::{
    get_block_slot_timestamp, get_latest_block_slot_at_timestamp, ConsensusCommandSender,
    ConsensusConfig, DiscardReason,
};
use crypto::{hash::Hash, signature::PublicKey};
use models::block::BlockHeader;
use serde::Deserialize;
use serde_json::json;
use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    net::IpAddr,
};
use time::UTime;
use tokio::sync::mpsc;
use warp::{filters::BoxedFilter, Filter, Rejection, Reply};

/// Events that are transmitted outside the API
#[derive(Debug, Copy, Clone)]
pub enum ApiEvent {
    /// API received stop signal and wants to fordward it
    AskStop,
}

pub enum ApiManagementCommand {}

#[derive(Debug, Deserialize, Clone, Copy)]
struct TimeInterval {
    start: UTime,
    end: UTime,
}

/// This function sets up all the routes that can be used
/// and combines them into one filter
///
pub fn get_filter(
    api_config: ApiConfig,
    consensus_config: ConsensusConfig,
    _protocol_config: ProtocolConfig,
    network_config: NetworkConfig,
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_command_sender: ConsensusCommandSender,
    _protocol_command_sender: ProtocolCommandSender,
    network_command_sender: NetworkCommandSender,
) -> BoxedFilter<(impl Reply,)> {
    let consensus_cmd = consensus_command_sender.clone();
    let block = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("block"))
        .and(warp::path::param::<Hash>()) //block hash
        .and(warp::path::end())
        .and_then(move |hash| get_block(consensus_cmd.clone(), hash));

    let consensus_cmd = consensus_command_sender.clone();
    let consensus_cfg = consensus_config.clone();
    let blockinterval = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("blockinterval"))
        .and(warp::query::<TimeInterval>()) //start, end
        .and(warp::path::end())
        .and_then(move |TimeInterval { start, end }| {
            get_block_interval(consensus_cmd.clone(), consensus_cfg.clone(), start, end)
        });

    let consensus_cmd = consensus_command_sender.clone();
    let current_parents = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("current_parents"))
        .and(warp::path::end())
        .and_then(move || get_current_parents(consensus_cmd.clone()));

    let consensus_cmd = consensus_command_sender.clone();
    let last_final = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_final"))
        .and(warp::path::end())
        .and_then(move || get_last_final(consensus_cmd.clone()));

    let consensus_cmd = consensus_command_sender.clone();
    let consensus_cfg = consensus_config.clone();
    let graph_interval = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("graph_interval"))
        .and(warp::query::<TimeInterval>()) //start, end //end
        .and(warp::path::end())
        .and_then(move |TimeInterval { start, end }| {
            get_graph_interval(consensus_cmd.clone(), consensus_cfg.clone(), start, end)
        });

    let consensus_cmd = consensus_command_sender.clone();
    let cliques = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("cliques"))
        .and(warp::path::end())
        .and_then(move || get_cliques(consensus_cmd.clone()));

    let network_cmd = network_command_sender.clone();
    let peers = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("peers"))
        .and(warp::path::end())
        .and_then(move || get_peers(network_cmd.clone()));

    let network_cfg = network_config.clone();
    let our_ip = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("our_ip"))
        .and_then(move || get_our_ip(network_cfg.clone()));

    let network_cmd = network_command_sender.clone();
    let network_cfg = network_config.clone();
    let network_info = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("network_info"))
        .and(warp::path::end())
        .and_then(move || get_network_info(network_cmd.clone(), network_cfg.clone()));

    let consensus_cmd = consensus_command_sender.clone();
    let network_cmd = network_command_sender.clone();
    let network_cfg = network_config.clone();
    let consensus_cfg = consensus_config.clone();
    let state = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("state"))
        .and(warp::path::end())
        .and_then(move || {
            get_state(
                consensus_cmd.clone(),
                network_cmd.clone(),
                consensus_cfg.clone(),
                network_cfg.clone(),
            )
        });

    let consensus_cmd = consensus_command_sender.clone();
    let api_cfg = api_config.clone();
    let last_stale = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_stale"))
        .and(warp::path::end())
        .and_then(move || get_last_stale(consensus_cmd.clone(), api_cfg.clone()));

    let consensus_cmd = consensus_command_sender.clone();
    let api_cfg = api_config.clone();
    let last_invalid = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_invalid"))
        .and(warp::path::end())
        .and_then(move || get_last_invalid(consensus_cmd.clone(), api_cfg.clone()));

    let consensus_cmd = consensus_command_sender.clone();
    let api_cfg = api_config.clone();
    let consensus_cfg = consensus_config.clone();
    let staker_info = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("staker_info"))
        .and(warp::path::param::<PublicKey>())
        .and(warp::path::end())
        .and_then(move |creator| {
            get_staker_info(
                consensus_cmd.clone(),
                api_cfg.clone(),
                consensus_cfg.clone(),
                creator,
            )
        });

    let evt_tx = event_tx.clone();
    let stop_node = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("stop_node"))
        .and(warp::path::end())
        .and_then(move || stop_node(evt_tx.clone()));

    block
        .or(blockinterval)
        .or(current_parents)
        .or(last_final)
        .or(graph_interval)
        .or(cliques)
        .or(peers)
        .or(our_ip)
        .or(network_info)
        .or(state)
        .or(last_stale)
        .or(last_invalid)
        .or(staker_info)
        .or(stop_node)
        .boxed()
}

/// This function sends AskStop outside the Api and
/// return the result as a warp reply.
///
/// # Argument
/// * event_tx : Sender used to send the event out
async fn stop_node(evt_tx: mpsc::Sender<ApiEvent>) -> Result<impl Reply, Rejection> {
    match evt_tx.send(ApiEvent::AskStop).await {
        Ok(_) => Ok(warp::reply().into_response()),
        Err(err) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error stopping node : {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
    }
}

/// Returns block with given hash as a reply
///
async fn get_block(
    consensus_command_sender: ConsensusCommandSender,
    hash: Hash,
) -> Result<impl Reply, Rejection> {
    match consensus_command_sender.get_active_block(hash).await {
        Err(err) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error retrieving active blocks : {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
        Ok(None) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("active block not found : {:?}", hash)
            })),
            warp::http::StatusCode::NOT_FOUND,
        )
        .into_response()),
        Ok(Some(block)) => Ok(warp::reply::json(&block).into_response()),
    }
}

/// Returns our ip adress
///
/// Note: as our ip adress is in the config,
/// this function is more about getting every bit of
/// information we want exactly in the same way
async fn get_our_ip(network_cfg: NetworkConfig) -> Result<impl warp::Reply, warp::Rejection> {
    Ok(warp::reply::json(&network_cfg.routable_ip))
}

/// Returns best parents as a Vec<Hash, (u64, u8)> wrapped in a reply.
/// The (u64, u8) tuple represents the parent's slot.
///
async fn get_current_parents(
    consensus_command_sender: ConsensusCommandSender,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match consensus_command_sender.get_block_graph_status().await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving graph : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(graph) => graph,
    };

    let parents = graph.best_parents;
    let mut best = Vec::new();
    for hash in parents {
        match graph.active_blocks.get_key_value(&hash) {
            Some((_, block)) => {
                best.push((hash, (block.block.period_number, block.block.thread_number)))
            }
            None => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&json!({
                        "message":
                            format!("inconsistency error between best_parents and active_blocks")
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response())
            }
        }
    }

    Ok(warp::reply::json(&best).into_response())
}

/// Returns last final blocks as a Vec<Hash, (u64, u8)> wrapped in a reply.
/// The (u64, u8) tuple represents the block's slot.
///
async fn get_last_final(
    consensus_command_sender: ConsensusCommandSender,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match consensus_command_sender.get_block_graph_status().await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving graph : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(graph) => graph,
    };
    let finals = graph
        .latest_final_blocks_periods
        .iter()
        .enumerate()
        .map(|(i, (hash, period))| (hash, *period, i as u8))
        .collect::<Vec<(&Hash, u64, u8)>>();
    Ok(warp::reply::json(&finals).into_response())
}

/// Returns all blocks in a time interval as a Vec<Hash, (u64, u8)> wrapped in a reply.
/// The (u64, u8) tuple represents the block's slot.
///
/// Note: both start time and end time are included
async fn get_block_interval(
    consensus_command_sender: ConsensusCommandSender,
    consensus_cfg: ConsensusConfig,
    start: UTime,
    end: UTime,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut res = Vec::new();
    let graph = match consensus_command_sender.get_block_graph_status().await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving graph : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(graph) => graph,
    };
    for (hash, exported_block) in graph.active_blocks {
        let header = exported_block.block;
        let time = match get_block_slot_timestamp(
            consensus_cfg.thread_count,
            consensus_cfg.t0,
            consensus_cfg.genesis_timestamp,
            (header.period_number, header.thread_number),
        ) {
            Ok(time) => time,
            Err(err) => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&json!({
                        "message": format!("error getting time : {:?}", err)
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response())
            }
        };
        if start <= time && time <= end {
            res.push((hash, (header.period_number, header.thread_number)));
        }
    }

    Ok(warp::reply::json(&res).into_response())
}

/// Returns all block info needed to reconstruct the graph found in the time interval.
/// The result is a vec of (hash, period, thread, status, parents hash) wrapped in a reply.
///
/// Note:
/// * both start time and end time are included
/// * status is in ["active", "final", "stale"]
async fn get_graph_interval(
    consensus_command_sender: ConsensusCommandSender,
    consensus_cfg: ConsensusConfig,
    start: UTime,
    end: UTime,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut res = HashMap::new();
    let graph = match consensus_command_sender.get_block_graph_status().await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving graph : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(graph) => graph,
    };
    for (hash, exported_block) in graph.active_blocks {
        let header = exported_block.block;
        let time = match get_block_slot_timestamp(
            consensus_cfg.thread_count,
            consensus_cfg.t0,
            consensus_cfg.genesis_timestamp,
            (header.period_number, header.thread_number),
        ) {
            Ok(time) => time,
            Err(err) => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&json!({
                        "message": format!("error getting time : {:?}", err)
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response())
            }
        };

        if start <= time && time <= end {
            res.insert(
                hash,
                (
                    header.period_number,
                    header.thread_number,
                    "active",
                    header.parents,
                ),
            );
        }
    }
    let mut final_blocks = HashMap::new();
    for (hash, (period, thread, _, parents)) in res.iter() {
        if !graph.gi_head.contains_key(&hash) {
            final_blocks.insert(hash.clone(), (*period, *thread, "final", parents.clone()));
        }
    }

    for (key, value) in final_blocks {
        res.insert(key, value);
    }

    for (hash, (reason, header)) in graph.discarded_blocks.map {
        let time = match get_block_slot_timestamp(
            consensus_cfg.thread_count,
            consensus_cfg.t0,
            consensus_cfg.genesis_timestamp,
            (header.period_number, header.thread_number),
        ) {
            Ok(time) => time,
            Err(err) => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&json!({
                        "message": format!("error getting time : {:?}", err)
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response())
            }
        };
        if start <= time && time <= end {
            let status;
            match reason {
                DiscardReason::Invalid => {
                    continue;
                }
                DiscardReason::Stale => status = "stale",
                DiscardReason::Final => status = "final",
            }
            res.insert(
                hash,
                (
                    header.period_number,
                    header.thread_number,
                    status,
                    header.parents,
                ),
            );
        }
    }
    let res = res
        .iter()
        .map(|(hash, (period, thread, status, parents))| {
            (hash.clone(), *period, *thread, *status, parents.clone())
        })
        .collect::<Vec<(Hash, u64, u8, &str, Vec<Hash>)>>();
    Ok(warp::reply::json(&res).into_response())
}

/// Returns number of cliques and current cliques as Vec<HashSet<(hash, (period, thread))>>
/// The result is a tuple (number_of_cliques, current_cliques) wrapped in a reply.
///
async fn get_cliques(
    consensus_command_sender: ConsensusCommandSender,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match consensus_command_sender.get_block_graph_status().await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving graph : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(graph) => graph,
    };

    let mut hashes = HashSet::new();
    for clique in graph.max_cliques.iter() {
        hashes.extend(clique)
    }

    let mut hashes_map = HashMap::new();
    for hash in hashes.iter() {
        if let Some((_, block)) = graph.active_blocks.get_key_value(hash) {
            hashes_map.insert(hash, (block.block.period_number, block.block.thread_number));
        } else {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("inconsticency error between cliques and active_blocks")
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response());
        }
    }

    let mut res = Vec::new();
    for clique in graph.max_cliques.iter() {
        let mut set = HashSet::new();
        for hash in clique.iter() {
            match hashes_map.get_key_value(hash) {
                Some((k, v)) => {
                    set.insert((k.clone(), v.clone()));
                }
                None => {
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&json!({
                            "message":"inconsistency error between clique and active blocks"
                        })),
                        warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                    )
                    .into_response())
                }
            }
        }
        res.push(set)
    }

    Ok(warp::reply::json(&(graph.max_cliques.len(), res)).into_response())
}

/// Returns network information:
/// * own IP address
/// * connected peers :
///      - ip address
///      - peer info (see PeerInfo struct in communication::network::PeerInfoDatabase)
///
async fn get_network_info(
    mut network_command_sender: NetworkCommandSender,
    network_cfg: NetworkConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    let peers = match network_command_sender.get_peers().await {
        Ok(peers) => peers,
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving peers : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    };
    let our_ip = network_cfg.routable_ip;
    Ok(warp::reply::json(&json!({
        "our_ip": our_ip,
        "peers": peers,
    }))
    .into_response())
}

/// Returns connected peers :
/// - ip address
/// - peer info (see PeerInfo struct in communication::network::PeerInfoDatabase)
///
async fn get_peers(
    mut network_command_sender: NetworkCommandSender,
) -> Result<impl warp::Reply, warp::Rejection> {
    let peers = match network_command_sender.get_peers().await {
        Ok(peers) => peers,
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving peers : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    };
    Ok(warp::reply::json(&peers).into_response())
}

/// Returns a summary of the current state:
/// * time in UTime
/// * lastest slot (optional)
/// * last final block as Vec<(&Hash, u64, u8, UTime)>
/// * number of cliques
/// * number of connected peers
///
async fn get_state(
    consensus_command_sender: ConsensusCommandSender,
    mut network_command_sender: NetworkCommandSender,
    consensus_cfg: ConsensusConfig,
    network_cfg: NetworkConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    let cur_time = match UTime::now() {
        Ok(time) => time,
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error getting current time : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    };

    let latest_slot_opt = match get_latest_block_slot_at_timestamp(
        consensus_cfg.thread_count,
        consensus_cfg.t0,
        consensus_cfg.genesis_timestamp,
        cur_time,
    ) {
        Ok(slot) => slot,
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error getting latest slot : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    };

    let peers = match network_command_sender.get_peers().await {
        Ok(peers) => peers,
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving peers : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    };

    let connected_peers: HashSet<IpAddr> = peers
        .iter()
        .filter(|(_ip, peer_info)| {
            peer_info.active_out_connections > 0 || peer_info.active_in_connections > 0
        })
        .map(|(ip, _peer_info)| *ip)
        .collect();

    let graph = match consensus_command_sender.get_block_graph_status().await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving graph : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(graph) => graph,
    };

    let finals = match graph
        .latest_final_blocks_periods
        .iter()
        .enumerate()
        .map(|(thread, (hash, period))| {
            Ok((
                hash,
                *period,
                thread as u8,
                get_block_slot_timestamp(
                    consensus_cfg.thread_count,
                    consensus_cfg.t0,
                    consensus_cfg.genesis_timestamp,
                    (*period, thread as u8),
                )?,
            ))
        })
        .collect::<Result<Vec<(&Hash, u64, u8, UTime)>, consensus::ConsensusError>>()
    {
        Ok(finals) => finals,
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error getting final blocks : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    };

    Ok(warp::reply::json(&json!({
        "time": cur_time,
        "latest_slot": latest_slot_opt,
        "our_ip": network_cfg.routable_ip,
        "last_final": finals,
        "nb_cliques": graph.max_cliques.len(),
        "nb_peers": connected_peers.len(),
    }))
    .into_response())
}

/// Returns a number of last stale blocks as a Vec<Hash, u64, u8> wrapped in a reply.
/// The (u64, u8) tuple represents the block's slot.
///
async fn get_last_stale(
    consensus_command_sender: ConsensusCommandSender,
    api_config: ApiConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match consensus_command_sender.get_block_graph_status().await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving graph : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(graph) => graph,
    };

    let discarded = graph.discarded_blocks.clone();
    let mut discarded = discarded
        .map
        .iter()
        .filter(|(_hash, (reason, _header))| *reason == DiscardReason::Stale)
        .map(|(hash, (_reason, header))| (hash, header.period_number, header.thread_number))
        .collect::<Vec<(&Hash, u64, u8)>>();
    if discarded.len() > 0 {
        let min = min(discarded.len(), api_config.max_return_invalid_blocks);
        discarded = discarded.drain(0..min).collect();
    }

    Ok(warp::reply::json(&json!(discarded)).into_response())
}

/// Returns a number of last invalid blocks as a Vec<Hash, u64, u8> wrapped in a reply.
/// The (u64, u8) tuple represents the block's slot.
///
async fn get_last_invalid(
    consensus_command_sender: ConsensusCommandSender,
    api_cfg: ApiConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match consensus_command_sender.get_block_graph_status().await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving graph : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(graph) => graph,
    };
    let discarded = graph.discarded_blocks.clone();

    let mut discarded = discarded
        .map
        .iter()
        .filter(|(_hash, (reason, _header))| *reason == DiscardReason::Invalid)
        .map(|(hash, (_reason, header))| (hash, header.period_number, header.thread_number))
        .collect::<Vec<(&Hash, u64, u8)>>();
    if discarded.len() > 0 {
        let min = min(discarded.len(), api_cfg.max_return_invalid_blocks);
        discarded = discarded.drain(0..min).collect();
    }

    Ok(warp::reply::json(&json!(discarded)).into_response())
}

/// Returns
/// * a number of discarded blocks by the staker as a Vec<(&Hash, DiscardReason, BlockHeader)>
/// * a number of active blocks by the staker as a Vec<(&Hash, BlockHeader)>
/// * next slots that are for the staker as a Vec<(u64, u8)>
///
async fn get_staker_info(
    consensus_command_sender: ConsensusCommandSender,
    api_cfg: ApiConfig,
    consensus_cfg: ConsensusConfig,
    creator: PublicKey,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match consensus_command_sender.get_block_graph_status().await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving graph : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(graph) => graph,
    };

    let blocks = graph
        .active_blocks
        .iter()
        .filter(|(_hash, block)| block.block.creator == creator)
        .map(|(hash, block)| (hash, block.block.clone()))
        .collect::<Vec<(&Hash, BlockHeader)>>();

    let discarded = graph
        .discarded_blocks
        .map
        .iter()
        .filter(|(_hash, (_reason, header))| header.creator == creator)
        .map(|(hash, (reason, header))| (hash, reason.clone(), header.clone()))
        .collect::<Vec<(&Hash, DiscardReason, BlockHeader)>>();
    let cur_time = match UTime::now() {
        Ok(time) => time,
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error getting current time : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    };

    let start_slot = match consensus::get_latest_block_slot_at_timestamp(
        consensus_cfg.thread_count,
        consensus_cfg.t0,
        consensus_cfg.genesis_timestamp,
        cur_time,
    ) {
        Ok(slot) => slot,
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error getting slot at timestamp : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    }
    .unwrap_or((0, 0));
    let end_slot = (
        start_slot
            .0
            .saturating_add(api_cfg.selection_return_periods),
        start_slot.1,
    );

    let next_slots_by_creator: Vec<(u64, u8)> = match consensus_command_sender
        .get_selection_draws(start_slot, end_slot)
        .await
    {
        Ok(slot) => slot,
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error selecting draw : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    }
    .into_iter()
    .filter_map(|(slt, sel)| {
        if sel == creator {
            return Some(slt);
        }
        None
    })
    .collect();

    Ok(warp::reply::json(&json!({
        "staker_active_blocks": blocks,
        "staker_discarded_blocks": discarded,
        "staker_next_draws": next_slots_by_creator,
    }))
    .into_response())
}
