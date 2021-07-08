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
//! Returns all blocks generate between start and end (excluded), in millis.
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
//! Returns some information on blocks generated between start and end (excluded), in millis :
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

use crate::ApiError;
use models::slot::Slot;

use super::config::ApiConfig;
use communication::{
    network::{NetworkCommandSender, NetworkConfig, PeerInfo},
    protocol::{ProtocolCommandSender, ProtocolConfig},
};
use consensus::{
    get_block_slot_timestamp, get_latest_block_slot_at_timestamp, time_range_to_slot_range,
    BlockGraphExport, ConsensusCommandSender, ConsensusConfig, ConsensusError, DiscardReason,
};
use crypto::{hash::Hash, signature::PublicKey};
use models::block::{Block, BlockHeader};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    net::IpAddr,
};
use storage::{StorageCommandSender, StorageConfig};
use time::UTime;
use tokio::sync::{mpsc, oneshot};
use warp::{filters::BoxedFilter, Filter, Rejection, Reply};

/// Events that are transmitted outside the API
#[derive(Debug)]
pub enum ApiEvent {
    /// API received stop signal and wants to fordward it
    AskStop,
    GetBlockGraphStatus(oneshot::Sender<BlockGraphExport>),
    GetActiveBlock(Hash, oneshot::Sender<Option<Block>>),
    GetPeers(oneshot::Sender<HashMap<IpAddr, PeerInfo>>),
    GetSelectionDraw(
        Slot,
        Slot,
        oneshot::Sender<Result<Vec<(Slot, PublicKey)>, ConsensusError>>,
    ),
}

pub enum ApiManagementCommand {}

#[derive(Debug, Deserialize, Clone, Copy)]
struct TimeInterval {
    start_time: Option<UTime>,
    end_time: Option<UTime>,
}

async fn wrap_api_call<F, T>(fut: F) -> Result<impl Reply, Rejection>
where
    F: std::future::Future<Output = Result<T, ApiError>>,
    T: Serialize,
{
    Ok(match fut.await {
        Ok(output) => {
            warp::reply::with_status(warp::reply::json(&output), warp::http::StatusCode::OK)
                .into_response()
        }
        Err(ApiError::NotFound) => warp::reply::with_status(
            warp::reply::json(&json!({
                "code": warp::http::StatusCode::NOT_FOUND.as_u16(),
                "message": "not found"
            })),
            warp::http::StatusCode::NOT_FOUND,
        )
        .into_response(),
        Err(e) => warp::reply::with_status(
            warp::reply::json(&json!({
                "code": warp::http::StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "message": e
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response(),
    })
}

/// This function sets up all the routes that can be used
/// and combines them into one filter
///
pub fn get_filter(
    api_config: ApiConfig,
    consensus_config: ConsensusConfig,
    _protocol_config: ProtocolConfig,
    network_config: NetworkConfig,
    _storage_config: StorageConfig,
    consensus_command_sender: ConsensusCommandSender,
    protocol_command_sender: ProtocolCommandSender,
    network_command_sender: NetworkCommandSender,
    storage_command_sender_opt: Option<StorageCommandSender>,
    event_tx: mpsc::Sender<ApiEvent>,
) -> BoxedFilter<(impl Reply,)> {
    let consensus_cmd = consensus_command_sender.clone();
    let storage_cmd_opt = storage_command_sender_opt.clone();
    let block = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("block"))
        .and(warp::path::param::<Hash>()) // block hash
        .and(warp::path::end())
        .and_then(move |hash| {
            wrap_api_call(get_block(
                consensus_cmd.clone(),
                storage_cmd_opt.clone(),
                hash,
            ))
        });

    let consensus_cfg = consensus_config.clone();
    let consensus_cmd = consensus_command_sender.clone();
    let storage_cmd_opt = storage_command_sender_opt.clone();
    let block_interval = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("graph"))
        .and(warp::path::end())
        .and(warp::query::<TimeInterval>()) // start, end
        .and_then(move |TimeInterval { start_time, end_time }| {
            wrap_api_call(get_graph(
                consensus_cfg.clone(),
                consensus_cmd.clone(),
                storage_cmd_opt.clone(),
                start_time,
                end_time,
            ))
        });
    



    







    let consensus_cfg = consensus_config.clone();
    let consensus_cmd = consensus_command_sender.clone();
    let storage_cmd_opt = storage_command_sender_opt.clone();
    let graph_interval = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("graph_interval"))
        .and(warp::path::end())
        .and(warp::query::<TimeInterval>()) //start, end
        .and_then(move |TimeInterval { start, end }| {
            get_graph_interval(
                consensus_cfg.clone(),
                consensus_cmd.clone(),
                storage_cmd_opt.clone(),
                start,
                end,
            )
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

    let consensus_cmd = event_tx.clone();
    let cliques = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("cliques"))
        .and(warp::path::end())
        .and_then(move || get_cliques(consensus_cmd.clone()));

    let network_cfg = network_config.clone();
    let network_cmd = network_command_sender.clone();
    let network_info = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("network_info"))
        .and(warp::path::end())
        .and_then(move || get_network_info(network_cfg.clone(), network_cmd.clone()));





        





        






    let evt_tx = event_tx.clone();
    let network_cfg = network_config.clone();
    let consensus_cfg = consensus_config.clone();
    let state = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("state"))
        .and(warp::path::end())
        .and_then(move || get_state(evt_tx.clone(), consensus_cfg.clone(), network_cfg.clone()));

    let evt_tx = event_tx.clone();
    let api_cfg = api_config.clone();
    let last_stale = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_stale"))
        .and(warp::path::end())
        .and_then(move || get_last_stale(evt_tx.clone(), api_cfg.clone()));

    let evt_tx = event_tx.clone();
    let api_cfg = api_config.clone();
    let last_invalid = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_invalid"))
        .and(warp::path::end())
        .and_then(move || get_last_invalid(evt_tx.clone(), api_cfg.clone()));

    let evt_tx = event_tx.clone();
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
                evt_tx.clone(),
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
        .or(block_interval)
        .or(current_parents)
        .or(last_final)
        .or(graph_interval)
        .or(cliques)
        .or(peers)
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

/// Returns block with given hash
///
async fn get_block(
    consensus_cmd: ConsensusCommandSender,
    storage_cmd_opt: Option<StorageCommandSender>,
    hash: Hash,
) -> Result<Block, ApiError> {
    match (consensus_cmd.get_active_block(hash).await?, storage_cmd_opt) {
        (Some(res), _) => res,
        (None, Some(storage_cmd)) => storage_cmd.get_block(hash).await?,
        (None, None) => None,
    }
    .ok_or(ApiError::NotFound)
}



//get_graph










/// Returns all blocks in a time interval as a Vec<(Slot, Hash)>
///
/// Note: start time is included and end time is excluded
async fn get_block_interval(
    consensus_cfg: ConsensusConfig,
    consensus_cmd: ConsensusCommandSender,
    storage_cmd_opt: Option<StorageCommandSender>,
    start_time: Option<UTime>,
    end_time: Option<UTime>,
) -> Result<serde_json::Value, ApiError> {
    // get bounds in terms of slots
    let (start_slot, end_slot) = time_range_to_slot_range(
        consensus_cfg.thread_count,
        consensus_cfg.t0,
        consensus_cfg.genesis_timestamp,
        start_time,
        end_time,
    )?;

    // list consensus blocks
    let mut res_map: HashMap<Hash, Slot> = consensus_cmd
        .get_block_graph_status()
        .await?
        .active_blocks
        .into_iter()
        .filter_map(|h, cb| {
            let slot = cb.block.slot;
            if let Some(b1) = start_slot {
                if slot < b1 {
                    return None;
                }
            }
            if let Some(b2) = end_slot {
                if slot >= b2 {
                    return None;
                }
            }
            Ok((h, slot))
        });

    // add storage blocks
    if let Some(storage_cmd) = storage_cmd_opt {
        res_map.extend(
            storage_cmd
                .get_hashes_in_slot_range(start_slot, end_slot)?
                .into_iter()
                .map(|s, h| (h, s)),
        );
    }

    // gather in Vec and sort by increasing slot, then by increasing hash if equality
    let mut res_vec = res_map.into_iter().map(|h, s| (s, h)).collect();
    res_vec.sort_unstable();
    res_vec
        .into_iter()
        .map(|(s, h)| {
            json!({
                "hash": h,
                "slot": s,
                "timestamp": get_block_slot_timestamp(
                    consensus_cfg.thread_count,
                    consensus_cfg.t0,
                    consensus_cfg.genesis_timestamp,
                    s
                )?
            })
        })
        .collect()
}


/// Returns best parents as a Vec<{Slot, Hash}> ordered by thread number
///
async fn get_current_parents(
    consensus_cmd: ConsensusCommandSender,
) -> Result<Vec<serde_json::Value>, ApiError> {
    let block_graph = consensus_cmd.get_block_graph_status()?;
    block_graph
        .best_parents
        .iter()
        .map(|h| Ok(json!({
            "slot": block_graph
                .active_blocks
                .get(&h)
                .ok_or(ApiError::DataInconsistencyError("best parent absent from active_blocks".into()))?
                .block
                .header
                .slot,
            "hash": h
        })))
        .collect()
}

/// Returns last final blocks as a Vec<(Slot, Hash)> in thread order.
///
async fn get_last_final(
    consensus_cmd: ConsensusCommandSender,
) -> Result<Vec<serde_json::Value>, ApiError> {
    let block_graph = consensus_cmd.get_block_graph_status()?;
    Ok(block_graph
        .latest_final_blocks_periods
        .iter()
        .enumerate()
        .map(|thread, (h, period)| {
            json!({
                "slot": Slot::new(period, thread as u8),
                "hash": h
            })
        })
        .collect())
}

/// Returns number of cliques and current cliques as Vec<Vec<{Slot, Hash}>>
/// Sorted by decreasing clique size
/// Each clique sorted by increasing (Slot, Hash)
async fn get_cliques(
    consensus_cmd: ConsensusCommandSender,
) -> Result<Vec<Vec<serde_json::Value>>, ApiError> {
    let block_graph = consensus_cmd.get_block_graph_status()?;
    let mut result: Vec<Vec<serde_json::Value>> = Vec::with_capacity(block_graph.max_cliques.len());
    for clique in block_graph.max_cliques {
        let mut res_clique: Vec<(Slot, Hash)> = Vec::with_capacity(clique.len());
        for h in clique {
            res_clique.push(json!({
                "slot": block_graph
                    .active_blocks
                    .get(&h)
                    .ok_or(ApiError::DataInconsistencyError("clique block absent from active_blocks".into()))?
                    .block
                    .header
                    .slot,
                "hash": h
            }));
        }
        res_clique.sort_unstable();
    }
    result.sort_unstable_by(|v| std::cmp::Reverse(v.len()));
    Ok(result)
}


/// Returns network information:
/// * own IP address
/// * connected peers :
///      - node_ip
///      - peer info
///
async fn get_network_info(
    network_cfg: NetworkConfig,
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<Vec<PeerInfo>, ApiError>  {
    Ok(json!({
        "node_ip": network_cfg.routable_ip,
        "peers": network_cmd
            .get_peers()?
            .values()
            .into_iter()
            .filter(|peer_info| (peer_info.active_out_connections > 0 || active_in_connections > 0))
            .collect()
    }))
}


































async fn retrieve_selection_draw(
    start: Slot,
    end: Slot,
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<Vec<(Slot, PublicKey)>, ApiError> {
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetSelectionDraw(start, end, response_tx))
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!(
                "Could not send api event get selection draw: {0}",
                e
            ))
        })?;
    response_rx
        .await
        .map_err(|e| {
            ApiError::ReceiveChannelError(format!("Could not retrieve selection draws: {0}", e))
        })?
        .map_err(|e| {
            ApiError::ReceiveChannelError(format!("Could not retrieve selection draws: {0}", e))
        })
}

async fn get_block_from_graph(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: &ConsensusConfig,
    start_opt: Option<UTime>,
    end_opt: Option<UTime>,
) -> Result<Vec<(Hash, Slot)>, String> {
    retrieve_graph_export(&event_tx)
        .await
        .map_err(|err| (format!("error retrieving graph : {:?}", err)))
        .and_then(|graph| {
            let start = start_opt.unwrap_or(UTime::from(0));
            let end = end_opt.unwrap_or(UTime::from(u64::MAX));

            graph
                .active_blocks
                .into_iter()
                .filter_map(|(hash, exported_block)| {
                    get_block_slot_timestamp(
                        consensus_cfg.thread_count,
                        consensus_cfg.t0,
                        consensus_cfg.genesis_timestamp,
                        exported_block.block.slot,
                    )
                    .map_err(|err| (format!("error getting time : {:?}", err)))
                    .map(|time| {
                        if start <= time && time < end {
                            Some((hash, exported_block.block.slot))
                        } else {
                            None
                        }
                    })
                    .transpose()
                })
                .collect::<Result<Vec<(Hash, Slot)>, String>>()
        })
}

async fn get_block_interval_process(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: ConsensusConfig,
    start_opt: Option<UTime>,
    end_opt: Option<UTime>,
    opt_storage_command_sender: Option<StorageCommandSender>,
) -> Result<Vec<(Hash, Slot)>, String> {
    if start_opt
        .and_then(|s| end_opt.and_then(|e| if s >= e { Some(()) } else { None }))
        .is_some()
    {
        return Ok(vec![]);
    }

    //filter block from graph_export
    let mut res = get_block_from_graph(event_tx, &consensus_cfg, start_opt, end_opt).await?;

    if let Some(ref storage) = opt_storage_command_sender {
        //add block from storage
        //get first slot

        let start_slot = if let Some(start) = start_opt {
            let slot = get_latest_block_slot_at_timestamp(
                consensus_cfg.thread_count,
                consensus_cfg.t0,
                consensus_cfg.genesis_timestamp,
                start,
            )
            .map_err(|err| (format!("error retrieving slot from time: {:?}", err)))
            .and_then(|opt_slot| {
                opt_slot
                    .map(|slot| {
                        get_block_slot_timestamp(
                            consensus_cfg.thread_count,
                            consensus_cfg.t0,
                            consensus_cfg.genesis_timestamp,
                            slot,
                        )
                        .map_err(|err| (format!("error retrieving next slot: {:?}", err)))
                        .and_then(|start_time| {
                            if start_time == start {
                                Ok(slot)
                            } else {
                                slot.get_next_slot(consensus_cfg.thread_count)
                                    .map_err(|err| format!("error retrieving next slot: {:?}", err))
                            }
                        })
                    })
                    .unwrap_or(Ok(Slot::new(0, 0)))
            })?;
            Some(slot)
        } else {
            None
        };

        //get end slot
        let end_slot_opt = if let Some(end) = end_opt {
            let slot = get_latest_block_slot_at_timestamp(
                consensus_cfg.thread_count,
                consensus_cfg.t0,
                consensus_cfg.genesis_timestamp,
                end,
            )
            .map_err(|err| (format!("error retrieving slot from time: {:?}", err)))
            .and_then(|opt_slot| {
                opt_slot
                    .map(|slot| {
                        get_block_slot_timestamp(
                            consensus_cfg.thread_count,
                            consensus_cfg.t0,
                            consensus_cfg.genesis_timestamp,
                            slot,
                        )
                        .map_err(|err| (format!("error retrieving next slot: {:?}", err)))
                        .and_then(|end_time| {
                            if end_time == end {
                                Ok(slot)
                            } else {
                                slot.get_next_slot(consensus_cfg.thread_count)
                                    .map_err(|err| format!("error retrieving next slot: {:?}", err))
                            }
                        })
                    })
                    .transpose()
            })?;
            Some(slot)
        } else {
            None
        };

        if let Some(end_slot) = end_slot_opt {
            storage
                .get_slot_range(start_slot, end_slot)
                .await
                .map(|blocks| {
                    res.append(
                        &mut blocks
                            .into_iter()
                            .map(|(hash, block)| (hash, block.header.slot))
                            .collect(),
                    )
                })
                .map_err(|err| (format!("error retrieving slot range: {:?}", err)))?;
        }
    }

    Ok(res)
}

async fn get_graph_interval_process(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: ConsensusConfig,
    start_opt: Option<UTime>,
    end_opt: Option<UTime>,
    opt_storage_command_sender: Option<StorageCommandSender>,
) -> Result<Vec<(Hash, Slot, String, Vec<Hash>)>, String> {
    //filter block from graph_export
    let mut res = retrieve_graph_export(&event_tx)
        .await
        .map_err(|err| (format!("error retrieving graph : {:?}", err)))
        .and_then(|graph| {
            let start = start_opt.unwrap_or(UTime::from(0));
            let end = end_opt.unwrap_or(UTime::from(u64::MAX));

            graph
                .active_blocks
                .into_iter()
                .filter_map(|(hash, exported_block)| {
                    let header = exported_block.block;
                    get_block_slot_timestamp(
                        consensus_cfg.thread_count,
                        consensus_cfg.t0,
                        consensus_cfg.genesis_timestamp,
                        header.slot,
                    )
                    .map_err(|err| (format!("error getting time : {:?}", err)))
                    .map(|time| {
                        if start <= time && time < end {
                            Some((hash, (header.slot, "active".to_string(), header.parents)))
                        } else {
                            None
                        }
                    })
                    .transpose()
                })
                .collect::<Result<Vec<(Hash, (Slot, String, Vec<Hash>))>, String>>()
        })?;

    if let Some(storage) = opt_storage_command_sender {
        let start_slot = if let Some(start) = start_opt {
            let start_slot = get_latest_block_slot_at_timestamp(
                consensus_cfg.thread_count,
                consensus_cfg.t0,
                consensus_cfg.genesis_timestamp,
                start,
            )
            .map_err(|err| (format!("error retrieving time : {:?}", err)))?;

            let start_slot = if let Some(slot) = start_slot {
                // if there is a slot at start timestamp
                if get_block_slot_timestamp(
                    consensus_cfg.thread_count,
                    consensus_cfg.t0,
                    consensus_cfg.genesis_timestamp,
                    slot,
                )
                .map_err(|err| (format!("error retrieving time : {:?}", err)))?
                    == start
                {
                    slot
                } else {
                    slot.get_next_slot(consensus_cfg.thread_count)
                        .map_err(|err| (format!("error retrieving graph : {:?}", err)))?
                }
            } else {
                // no slot found
                Slot::new(0, 0)
            };
            Some(start_slot)
        } else {
            None
        };

        let end_slot = if let Some(end) = end_opt {
            let end_slot = get_latest_block_slot_at_timestamp(
                consensus_cfg.thread_count,
                consensus_cfg.t0,
                consensus_cfg.genesis_timestamp,
                end,
            )
            .map_err(|err| (format!("error retrieving time : {:?}", err)))?;

            let end_slot = if let Some(slot) = end_slot {
                if get_block_slot_timestamp(
                    consensus_cfg.thread_count,
                    consensus_cfg.t0,
                    consensus_cfg.genesis_timestamp,
                    slot,
                )
                .map_err(|err| (format!("error retrieving time : {:?}", err)))?
                    == end
                {
                    slot
                } else {
                    slot.get_next_slot(consensus_cfg.thread_count)
                        .map_err(|err| (format!("error retrieving time : {:?}", err)))?
                }
            } else {
                return Err("No end timestamp".to_string());
            };
            Some(end_slot)
        } else {
            None
        };
        let blocks = storage
            .get_slot_range(start_slot, end_slot)
            .await
            .map_err(|err| (format!("error retrieving time : {:?}", err)))?;
        for (hash, block) in blocks {
            res.push((hash, (block.header.slot, "final".to_string(), Vec::new())));
        }
    }
    Ok(res
        .into_iter()
        .map(|(hash, (slot, status, parents))| (hash, slot, status, parents))
        .collect())
}


/// Returns a summary of the current state:
/// * time in UTime
/// * lastest slot (optional)
/// * last final block as Vec<(&Hash, Slot UTime)>
/// * number of cliques
/// * number of connected peers
///
async fn get_state(
    event_tx: mpsc::Sender<ApiEvent>,
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

    let peers = match retrieve_peers(&event_tx).await {
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

    let graph = match retrieve_graph_export(&event_tx).await {
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
                Slot::new(*period, thread as u8),
                get_block_slot_timestamp(
                    consensus_cfg.thread_count,
                    consensus_cfg.t0,
                    consensus_cfg.genesis_timestamp,
                    Slot::new(*period, thread as u8),
                )?,
            ))
        })
        .collect::<Result<Vec<(&Hash, Slot, UTime)>, consensus::ConsensusError>>()
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

/// Returns a number of last stale blocks as a Vec<(Hash, Slot)> wrapped in a reply.
///
async fn get_last_stale(
    event_tx: mpsc::Sender<ApiEvent>,
    api_config: ApiConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match retrieve_graph_export(&event_tx).await {
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
        .map(|(hash, (_reason, header))| (hash, header.slot))
        .collect::<Vec<(&Hash, Slot)>>();
    if discarded.len() > 0 {
        let min = min(discarded.len(), api_config.max_return_invalid_blocks);
        discarded = discarded.drain(0..min).collect();
    }

    Ok(warp::reply::json(&json!(discarded)).into_response())
}

/// Returns a number of last invalid blocks as a Vec<(Hash, Slot)> wrapped in a reply.
///
async fn get_last_invalid(
    event_tx: mpsc::Sender<ApiEvent>,
    api_cfg: ApiConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match retrieve_graph_export(&event_tx).await {
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
        .map(|(hash, (_reason, header))| (hash, header.slot))
        .collect::<Vec<(&Hash, Slot)>>();
    if discarded.len() > 0 {
        let min = min(discarded.len(), api_cfg.max_return_invalid_blocks);
        discarded = discarded.drain(0..min).collect();
    }

    Ok(warp::reply::json(&json!(discarded)).into_response())
}

/// Returns
/// * a number of discarded blocks by the staker as a Vec<(&Hash, DiscardReason, BlockHeader)>
/// * a number of active blocks by the staker as a Vec<(&Hash, BlockHeader)>
/// * next slots that are for the staker as a Vec<Slot>
///
async fn get_staker_info(
    event_tx: mpsc::Sender<ApiEvent>,
    api_cfg: ApiConfig,
    consensus_cfg: ConsensusConfig,
    creator: PublicKey,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = match retrieve_graph_export(&event_tx).await {
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
    .unwrap_or(Slot::new(0, 0));
    let end_slot = Slot::new(
        start_slot
            .period
            .saturating_add(api_cfg.selection_return_periods),
        start_slot.thread,
    );

    let next_slots_by_creator: Vec<Slot> =
        match retrieve_selection_draw(start_slot, end_slot, &event_tx).await {
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
