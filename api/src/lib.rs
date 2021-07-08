/// The goal of this API is to retrieve information
/// on the current state of our node and interact with it.
/// In version 0.1, we can get some informations
/// and stop the node through the API.
use communication::network::config::NetworkConfig;
use config::ApiConfig;
use consensus::{config::ConsensusConfig, consensus_controller::ConsensusControllerInterface};
use consensus::{get_block_slot_timestamp, DiscardReason};
use crypto::{hash::Hash, signature::PublicKey};
use models::block::BlockHeader;
use serde_json::json;
use std::cmp::min;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use time::UTime;
use tokio::sync::{mpsc, oneshot};
use warp::{filters::BoxedFilter, Filter, Rejection, Reply};

pub mod config;
pub mod error;

pub use error::*;

/// This function sets up all the routes that can be used
/// and combines them into one filter
///
/// # Arguments
/// * consensus_controller_interface: the only communication channel we have with the node
/// * api_config, consensus_config, network_config : configuration that are needed here
///     (for more details see config.rs file in each crate)
/// * evt_tx : channel used to comminicate ApiEvents
pub fn get_filter<ConsensusControllerInterfaceT: ConsensusControllerInterface + 'static>(
    consensus_controller_interface: ConsensusControllerInterfaceT,
    api_config: ApiConfig,
    consensus_config: ConsensusConfig,
    network_config: NetworkConfig,
    evt_tx: mpsc::Sender<ApiEvent>,
) -> BoxedFilter<(impl Reply,)> {
    let interface = consensus_controller_interface.clone();
    let block = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("block"))
        .and(warp::path::param::<Hash>()) //block hash
        .and(warp::path::end())
        .and_then(move |hash| get_block(hash, interface.clone()));

    let interface = consensus_controller_interface.clone();
    let cfg = consensus_config.clone();
    let blockinterval = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("blockinterval"))
        .and(warp::path::param::<UTime>()) //start
        .and(warp::path::param::<UTime>()) //end
        .and(warp::path::end())
        .and_then(move |start, end| block_interval(interface.clone(), cfg.clone(), start, end));

    let interface = consensus_controller_interface.clone();
    let current_parents = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("current_parents"))
        .and(warp::path::end())
        .and_then(move || current_parents(interface.clone()));

    let interface = consensus_controller_interface.clone();
    let last_final = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_final"))
        .and(warp::path::end())
        .and_then(move || last_final(interface.clone()));

    let interface = consensus_controller_interface.clone();
    let cfg = consensus_config.clone();
    let graph_interval = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("graph_interval"))
        .and(warp::path::param::<UTime>()) //start
        .and(warp::path::param::<UTime>()) //end
        .and(warp::path::end())
        .and_then(move |start, end| graph_interval(interface.clone(), cfg.clone(), start, end));

    let interface = consensus_controller_interface.clone();
    let cliques = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("cliques"))
        .and(warp::path::end())
        .and_then(move || cliques(interface.clone()));

    let interface = consensus_controller_interface.clone();
    let peers = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("peers"))
        .and(warp::path::end())
        .and_then(move || peers(interface.clone()));

    let cfg = network_config.clone();
    let our_ip = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("our_ip"))
        .and_then(move || our_ip(cfg.clone()));

    let interface = consensus_controller_interface.clone();
    let cfg = network_config.clone();
    let network_info = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("network_info"))
        .and(warp::path::end())
        .and_then(move || network_info(interface.clone(), cfg.clone()));

    let interface = consensus_controller_interface.clone();
    let network_cfg = network_config.clone();
    let consensus_cfg = consensus_config.clone();
    let state = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("state"))
        .and(warp::path::end())
        .and_then(move || {
            state(
                interface.clone(),
                consensus_cfg.clone(),
                network_cfg.clone(),
            )
        });

    let interface = consensus_controller_interface.clone();
    let api_cfg = api_config.clone();
    let last_stale = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_stale"))
        .and(warp::path::end())
        .and_then(move || last_stale(api_cfg.clone(), interface.clone()));

    let interface = consensus_controller_interface.clone();
    let api_cfg = api_config.clone();
    let last_invalid = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_invalid"))
        .and(warp::path::end())
        .and_then(move || last_invalid(api_cfg.clone(), interface.clone()));

    let interface = consensus_controller_interface.clone();
    let api_cfg = api_config.clone();
    let consensus_cfg = consensus_config.clone();
    let staker_info = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("staker_info"))
        .and(warp::path::param::<PublicKey>())
        .and(warp::path::end())
        .and_then(move |creator| {
            staker_info(
                creator,
                api_cfg.clone(),
                consensus_cfg.clone(),
                interface.clone(),
            )
        });

    let event_tx = evt_tx.clone();
    let stop_node = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("stop_node"))
        .and(warp::path::end())
        .and_then(move || stop_node(event_tx.clone()));

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
        .or_else(|_| async move { Err(warp::reject::not_found()) })
        .boxed()
}

/// Structure used to communicate with the api
/// for example, we want to be able to stop the
/// node from outside or inside the api
pub struct ApiHandle {
    /// channel used to gracefully shutdown the api
    stop_tx: oneshot::Sender<()>,
    /// used to join on the API
    join_handle: tokio::task::JoinHandle<()>,
    /// used to transmit ApiEvents outside
    evt_rx: mpsc::Receiver<ApiEvent>,
}

/// Events that are transmitted outside the API
#[derive(Debug, Copy, Clone)]
pub enum ApiEvent {
    /// API received stop signal and wants to fordward it
    AskStop,
}

impl ApiHandle {
    /// Shut down the Apit properly
    pub async fn stop(self) -> Result<(), ApiError> {
        self.stop_tx
            .send(())
            .map_err(|_| ApiError::SendChannelError("cannot send stop command".into()))?;
        self.join_handle
            .await
            .map_err(|err| ApiError::JoinError(err))?;
        Ok(())
    }

    /// Listen for ApiEvents
    pub async fn wait_event(&mut self) -> Result<ApiEvent, ApiError> {
        self.evt_rx
            .recv()
            .await
            .ok_or(ApiError::SendChannelError(format!(
                "could not receive api event"
            )))
    }
}

/// Spawn API server.
///
/// # Arguments
/// * consensus_controller_interface: the only communication channel we have with the node
/// * api_config, consensus_config, network_config : configuration that are needed here
///     (for more details see config.rs file in each crate)
///
/// Returns the ApiHandle to keep a communication channel with the Api
pub async fn spawn_server<ConsensusControllerInterfaceT: ConsensusControllerInterface + 'static>(
    consensus_controller_interface: ConsensusControllerInterfaceT,
    api_config: ApiConfig,
    consensus_config: ConsensusConfig,
    network_config: NetworkConfig,
) -> Result<ApiHandle, warp::Error> {
    let (stop_tx, stop_rx) = oneshot::channel();
    let (evt_tx, evt_rx) = mpsc::channel(1024);

    let (_addr, server) = warp::serve(get_filter(
        consensus_controller_interface,
        api_config.clone(),
        consensus_config,
        network_config,
        evt_tx,
    ))
    .try_bind_with_graceful_shutdown(api_config.bind.clone(), async {
        stop_rx.await.ok();
    })?;

    let join_handle = tokio::task::spawn(server);

    Ok(ApiHandle {
        stop_tx,
        join_handle,
        evt_rx,
    })
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
/// # Arguments
/// * hash: the hash of the block we want
/// * consensus_controller_interface: the only communication channel we have with the node
async fn get_block<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    hash: Hash,
    interface: ConsensusControllerInterfaceT,
) -> Result<impl Reply, Rejection> {
    match interface.get_active_block(hash).await {
        Err(err) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error retrieving active blocks : {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
        Ok(None) => Err(warp::reject::not_found()),
        Ok(Some(block)) => Ok(warp::reply::json(&block).into_response()),
    }
}

/// Returns our ip adress
///
/// # Argument
/// * cfg: Network configuration
///
/// Note: as our ip adress is in the config,
/// this function is more about getting every bit of
/// information we want exactly in the same way
async fn our_ip(cfg: NetworkConfig) -> Result<impl warp::Reply, warp::Rejection> {
    Ok(warp::reply::json(&cfg.routable_ip))
}

/// Returns best parents as a Vec<Hash, (u64, u8)> wrapped in a reply.
/// The (u64, u8) tuple represents the parent's slot.
///
/// # Argument
/// * consensus_controller_interface: the only communication channel we have with the node
async fn current_parents<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match interface.get_block_graph_status().await {
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
/// # Argument
/// * consensus_controller_interface: the only communication channel we have with the node
async fn last_final<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match interface.get_block_graph_status().await {
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
/// # Arguments
/// * consensus_controller_interface: the only communication channel we have with the node
/// * cfg: consensus configuration
/// * start: beginning of the considered interval
/// * end: ending of the considered interval
///
/// Note: both start time and end time are included
async fn block_interval<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
    cfg: ConsensusConfig,
    start: UTime,
    end: UTime,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut res = Vec::new();
    let graph = match interface.get_block_graph_status().await {
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
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
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
/// # Arguments
/// * consensus_controller_interface: the only communication channel we have with the node
/// * cfg: consensus configuration
/// * start: beginning of the considered interval
/// * end: ending of the considered interval
///
/// Note:
/// * both start time and end time are included
/// * status is in ["active", "final", "stale"]
async fn graph_interval<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
    cfg: ConsensusConfig,
    start: UTime,
    end: UTime,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut res = HashMap::new();
    let graph = match interface.get_block_graph_status().await {
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
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
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
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
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
/// # Arguments
/// * consensus_controller_interface: the only communication channel we have with the node
async fn cliques<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match interface.get_block_graph_status().await {
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
/// # Arguments
/// * consensus_controller_interface: the only communication channel we have with the node
/// * cfg: network configuration
async fn network_info<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
    cfg: NetworkConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    let peers = match interface.get_peers().await {
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
    let our_ip = cfg.routable_ip;
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
/// # Arguments
/// * consensus_controller_interface: the only communication channel we have with the node
async fn peers<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let peers = match interface.get_peers().await {
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
/// # Arguments
/// * consensus_controller_interface: the only communication channel we have with the node
/// * network_cfg:  network configuration
/// * consensus_cfg: consensus configuration
async fn state<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
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

    let latest_slot_opt = match consensus::get_latest_block_slot_at_timestamp(
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

    let peers = match interface.get_peers().await {
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

    let graph = match interface.get_block_graph_status().await {
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
/// # Arguments
/// * api_config: api configuration
/// * consensus_controller_interface: the only communication channel we have with the node
async fn last_stale<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    api_config: ApiConfig,
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match interface.get_block_graph_status().await {
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
/// # Arguments
/// * api_config: api configuration
/// * consensus_controller_interface: the only communication channel we have with the node
async fn last_invalid<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    api_config: ApiConfig,
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match interface.get_block_graph_status().await {
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
        let min = min(discarded.len(), api_config.max_return_invalid_blocks);
        discarded = discarded.drain(0..min).collect();
    }

    Ok(warp::reply::json(&json!(discarded)).into_response())
}

/// Returns
/// * a number of discarded blocks by the staker as a Vec<(&Hash, DiscardReason, BlockHeader)>
/// * a number of active blocks by the staker as a Vec<(&Hash, BlockHeader)>
/// * next slots that are for the staker as a Vec<(u64, u8)>
///
/// # Arguments
/// * creator: PublicKey of the staker we are interested in
/// * api_config: api configuration
/// * consensus_cfg: consensus configuration
/// * consensus_controller_interface: the only communication channel we have with the node
async fn staker_info<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    creator: PublicKey,
    api_cfg: ApiConfig,
    consensus_cfg: ConsensusConfig,
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match interface.get_block_graph_status().await {
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

    let next_slots_by_creator: Vec<(u64, u8)> =
        match interface.get_selection_draws(start_slot, end_slot).await {
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

#[cfg(test)]
mod test;
