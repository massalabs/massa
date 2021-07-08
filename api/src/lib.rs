use communication::network::config::NetworkConfig;
use config::ApiConfig;
use consensus::{config::ConsensusConfig, consensus_controller::ConsensusControllerInterface};
use consensus::{get_block_slot_timestamp, DiscardReason};
use crypto::{hash::Hash, signature::PublicKey};
use models::block::BlockHeader;
use serde_json::json;
use std::{cmp::min, collections::HashSet, net::IpAddr};
use time::UTime;
use warp::{filters::BoxedFilter, reject, Filter, Rejection, Reply};

pub mod config;

#[derive(Debug)]
struct InternalError {
    message: String,
}

impl reject::Reject for InternalError {}

pub fn get_filter<ConsensusControllerInterfaceT: ConsensusControllerInterface + 'static>(
    consensus_controller_interface: ConsensusControllerInterfaceT,
    api_config: ApiConfig,
    consensus_config: ConsensusConfig,
    network_config: NetworkConfig,
) -> BoxedFilter<(impl Reply,)> {
    let interface = consensus_controller_interface.clone();
    let block = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("block"))
        .and(warp::path::param::<Hash>()) //block hash
        //.and(warp::path::end())
        .and_then(move |hash| get_block(hash, interface.clone()));

    let interface = consensus_controller_interface.clone();
    let cfg = consensus_config.clone();
    let blockinterval = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("blockinterval"))
        .and(warp::path::param::<UTime>()) //start
        .and(warp::path::param::<UTime>()) //end
        //.and(warp::path::end())
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
        //.and(warp::path::end())
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
        //.and(warp::path::end())
        .and_then(move || network_info(interface.clone(), cfg.clone()));

    let interface = consensus_controller_interface.clone();
    let network_cfg = network_config.clone();
    let consensus_cfg = consensus_config.clone();
    let state = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("state"))
        //.and(warp::path::end())
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
        //.and(warp::path::end())
        .and_then(move || last_stale(api_cfg.clone(), interface.clone()));

    let interface = consensus_controller_interface.clone();
    let api_cfg = api_config.clone();
    let last_invalid = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_invalid"))
        //.and(warp::path::end())
        .and_then(move || last_invalid(api_cfg.clone(), interface.clone()));

    let interface = consensus_controller_interface.clone();
    let api_cfg = api_config.clone();
    let consensus_cfg = consensus_config.clone();
    let staker_info = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("staker_info"))
        .and(warp::path::param::<PublicKey>())
        //.and(warp::path::end())
        .and_then(move |creator| {
            staker_info(
                creator,
                api_cfg.clone(),
                consensus_cfg.clone(),
                interface.clone(),
            )
        });

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
        .boxed()
}

pub async fn serve<ConsensusControllerInterfaceT: ConsensusControllerInterface + 'static>(
    consensus_controller_interface: ConsensusControllerInterfaceT,
    api_config: ApiConfig,
    consensus_config: ConsensusConfig,
    network_config: NetworkConfig,
) {
    warp::serve(get_filter(
        consensus_controller_interface,
        api_config,
        consensus_config,
        network_config,
    ))
    .run(([127, 0, 0, 1], 3030))
    .await;
}

async fn get_block<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    hash: Hash,
    interface: ConsensusControllerInterfaceT,
) -> Result<impl Reply, Rejection> {
    match interface.get_active_block(hash).await {
        Err(err) => Err(warp::reject::custom(InternalError {
            message: err.to_string(),
        })),
        Ok(None) => Err(warp::reject::not_found()),
        Ok(Some(block)) => Ok(warp::reply::json(&block)),
    }
}

async fn our_ip(cfg: NetworkConfig) -> Result<impl warp::Reply, warp::Rejection> {
    Ok(warp::reply::json(&cfg.routable_ip))
}

async fn current_parents<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let parents = interface
        .get_block_graph_status()
        .await
        .map_err(|err| InternalError {
            message: format!("error get block gaph: {:#?}", err),
        })?
        .best_parents;
    Ok(warp::reply::json(&parents))
}

async fn last_final<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let finals = interface
        .get_block_graph_status()
        .await
        .map_err(|err| InternalError {
            message: format!("error get block gaph: {:#?}", err),
        })?;
    let finals = finals
        .latest_final_blocks_periods
        .iter()
        .enumerate()
        .map(|(i, (hash, period))| (hash, *period, i as u8))
        .collect::<Vec<(&Hash, u64, u8)>>();
    Ok(warp::reply::json(&finals))
}

/// return all block hash found in the time interval
/// both start time and end time are included
async fn block_interval<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
    cfg: ConsensusConfig,
    start: UTime,
    end: UTime,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut res = HashSet::new();
    let graph = interface
        .get_block_graph_status()
        .await
        .map_err(|err| InternalError {
            message: format!("error get block gaph: {:#?}", err),
        })?;
    for (hash, exported_block) in graph.active_blocks {
        let header = exported_block.block;
        let time = get_block_slot_timestamp(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            (header.period_number, header.thread_number),
        )
        .map_err(|err| InternalError {
            message: format!("timestamp error: {:#?}", err),
        })?;
        if start <= time && time <= end {
            res.insert(hash);
        }
    }
    Ok(warp::reply::json(&res))
}

/// return all block info needed to reconstruct the graph found in the time interval
/// -> list of (hash, thread, slot, status, parents hash)
/// both start time and end time are included
async fn graph_interval<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
    cfg: ConsensusConfig,
    start: UTime,
    end: UTime,
) -> Result<impl warp::Reply, warp::Rejection> {
    let mut res = HashSet::new();
    let graph = interface
        .get_block_graph_status()
        .await
        .map_err(|err| InternalError {
            message: format!("error get block gaph: {:#?}", err),
        })?;
    for (hash, exported_block) in graph.active_blocks {
        let header = exported_block.block;
        let time = get_block_slot_timestamp(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            (header.period_number, header.thread_number),
        )
        .map_err(|err| InternalError {
            message: format!("timestamp error: {:#?}", err),
        })?;
        if start <= time && time <= end {
            res.insert((
                hash,
                header.period_number,
                header.thread_number,
                "valid",
                header.parents,
            ));
        }
    }
    Ok(warp::reply::json(&res))
}

// async fn last_stale<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
//     interface: ConsensusControllerInterfaceT,
// ) -> Result<impl warp::Reply, warp::Rejection> {
//     unimplemented!()
// }

/// return number of cliques and current cliques hashset set
async fn cliques<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = interface
        .get_block_graph_status()
        .await
        .map_err(|err| InternalError {
            message: format!("error get block gaph: {:#?}", err),
        })?;
    Ok(warp::reply::json(&(
        graph.max_cliques.len(),
        graph.max_cliques,
    )))
}

/// return network information: own IP address, connected peers (address, IP), connection status, connections usage, average upload and download bandwidth used for headers, operations, syncing
async fn network_info<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
    cfg: NetworkConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    let peers = interface.get_peers().await.map_err(|err| InternalError {
        message: format!("error get peers: {:#?}", err),
    })?;
    let our_ip = cfg.routable_ip;
    Ok(warp::reply::json(&json!({
        "our_ip": our_ip,
        "peers": peers,
    })))
}

async fn peers<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let peers = interface.get_peers().await.map_err(|err| InternalError {
        message: format!("get peers error: {:#?}", err),
    })?;
    Ok(warp::reply::json(&peers))
}

/// returns a summary of the current state: time, last final block (hash, thread, slot, timestamp), nb cliques, nb connected nodes
async fn state<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
    consensus_cfg: ConsensusConfig,
    network_cfg: NetworkConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    let cur_time = UTime::now().map_err(|err| InternalError {
        message: format!("error getting current time: {:#?}", err),
    })?;

    let latest_slot_opt = consensus::get_latest_block_slot_at_timestamp(
        consensus_cfg.thread_count,
        consensus_cfg.t0,
        consensus_cfg.genesis_timestamp,
        cur_time,
    )
    .map_err(|err| InternalError {
        message: format!("get_latest_block_slot_at_timestamp error: {:#?}", err),
    })?;

    let peers = interface.get_peers().await.map_err(|err| InternalError {
        message: format!("get peers error: {:#?}", err),
    })?;

    let connected_peers: HashSet<IpAddr> = peers
        .iter()
        .filter(|(_ip, peer_info)| {
            peer_info.active_out_connections > 0 || peer_info.active_in_connections > 0
        })
        .map(|(ip, _peer_info)| *ip)
        .collect();

    let graph = interface
        .get_block_graph_status()
        .await
        .map_err(|err| InternalError {
            message: format!("get_block_graph_status error: {:#?}", err),
        })?;

    let finals = graph
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
        .map_err(|err| InternalError {
            message: format!("error gathering final blocks: {:#?}", err),
        })?;

    Ok(warp::reply::json(&json!({
        "time": cur_time,
        "latest_slot": latest_slot_opt,
        "our_ip": network_cfg.routable_ip,
        "last_final": finals,
        "nb_cliques": graph.max_cliques.len(),
        "nb_peers": connected_peers.len(),
    })))
}

async fn last_stale<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    api_config: ApiConfig,
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = interface
        .get_block_graph_status()
        .await
        .map_err(|err| InternalError {
            message: format!("error get block gaph: {:#?}", err),
        })?;

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

    Ok(warp::reply::json(&json!(discarded)))
}

async fn last_invalid<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    api_config: ApiConfig,
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = interface
        .get_block_graph_status()
        .await
        .map_err(|err| InternalError {
            message: format!("error get block gaph: {:#?}", err),
        })?;

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

    Ok(warp::reply::json(&json!(discarded)))
}

async fn staker_info<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    creator: PublicKey,
    api_cfg: ApiConfig,
    consensus_cfg: ConsensusConfig,
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let graph = interface
        .get_block_graph_status()
        .await
        .map_err(|err| InternalError {
            message: format!("error get block gaph: {:#?}", err),
        })?;

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
    let cur_time = UTime::now().map_err(|err| InternalError {
        message: format!("error getting current time: {:#?}", err),
    })?;

    let start_slot = consensus::get_latest_block_slot_at_timestamp(
        consensus_cfg.thread_count,
        consensus_cfg.t0,
        consensus_cfg.genesis_timestamp,
        cur_time,
    )
    .map_err(|err| InternalError {
        message: format!("get_latest_block_slot_at_timestamp error: {:#?}", err),
    })?
    .unwrap_or((0, 0));
    let end_slot = (
        start_slot
            .0
            .saturating_add(api_cfg.selection_return_periods),
        start_slot.1,
    );

    let next_slots_by_creator: Vec<(u64, u8)> = interface
        .get_selection_draws(start_slot, end_slot)
        .await
        .map_err(|err| InternalError {
            message: format!("get_creator interface error: {:#?}", err),
        })?
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
    })))
}

#[cfg(test)]
mod test;
