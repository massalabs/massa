use communication::network::config::NetworkConfig;
use consensus::get_block_slot_timestamp;
use consensus::{config::ConsensusConfig, consensus_controller::ConsensusControllerInterface};
use crypto::hash::Hash;
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
};
use time::UTime;
use warp::{filters::BoxedFilter, reject, Filter, Rejection, Reply};

#[derive(Debug)]
struct InternalError {
    message: String,
}

impl reject::Reject for InternalError {}

pub fn get_filter<ConsensusControllerInterfaceT: ConsensusControllerInterface + 'static>(
    consensus_controller_interface: ConsensusControllerInterfaceT,
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
        .boxed()
}

pub async fn serve<ConsensusControllerInterfaceT: ConsensusControllerInterface + 'static>(
    consensus_controller_interface: ConsensusControllerInterfaceT,
    consensus_config: ConsensusConfig,
    network_config: NetworkConfig,
) {
    warp::serve(get_filter(
        consensus_controller_interface,
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
        .map_err(|_| warp::reject::reject())?
        .best_parents;
    Ok(warp::reply::json(&parents))
}

async fn last_final<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let finals = interface
        .get_block_graph_status()
        .await
        .map_err(|_| warp::reject::reject())?;
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
        .map_err(|_| warp::reject::reject())?;
    for (hash, exported_block) in graph.active_blocks {
        let header = exported_block.block;
        let time = get_block_slot_timestamp(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            (header.period_number, header.thread_number),
        )
        .map_err(|_| warp::reject::reject())?;
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
        .map_err(|_| warp::reject::reject())?;
    for (hash, exported_block) in graph.active_blocks {
        let header = exported_block.block;
        let time = get_block_slot_timestamp(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            (header.period_number, header.thread_number),
        )
        .map_err(|_| warp::reject::reject())?;
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
        .map_err(|_| warp::reject::reject())?;
    Ok(warp::reply::json(&(
        graph.max_cliques.len(),
        graph.max_cliques,
    )))
}

async fn peers<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let peers = interface
        .get_peers()
        .await
        .map_err(|_| warp::reject::reject())?;
    Ok(warp::reply::json(&peers))
}

/// return network information: own IP address, connected peers (address, IP), connection status, connections usage, average upload and download bandwidth used for headers, operations, syncing
async fn network_info<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
    cfg: NetworkConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    let peers = interface
        .get_peers()
        .await
        .map_err(|_| warp::reject::reject())?;
    let our_ip = cfg.routable_ip;
    Ok(warp::reply::json(&json!({
        "our_ip": our_ip,
        "peers": peers,
    })))
}

/// returns a summary of the current state: time, last final block (hash, thread, slot, timestamp), nb cliques, nb connected nodes
async fn state<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
    consensus_cfg: ConsensusConfig,
    network_cfg: NetworkConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    let time = UTime::now().unwrap(); //.map_err(|err | Err(warp::reject::custom(InternalError {message: "time error".into()})))?;
    let time = consensus::get_latest_block_slot_at_timestamp(
        consensus_cfg.thread_count,
        consensus_cfg.t0,
        consensus_cfg.genesis_timestamp,
        time,
    ) /*.map_err(|err | Err(warp::reject::custom(InternalError {message: "time error".into()})))*/
    .unwrap() /*?*/
    .ok_or(InternalError {
        message: "No own ip".into(),
    })?;
    let peers: HashMap<IpAddr, String> = interface
        .get_peers()
        .await
        .map_err(|_| warp::reject::reject())?;

    let connected_peers: HashSet<IpAddr> = peers
        .iter()
        .filter(|(_ip, status)| status.as_str() == "connected")
        .map(|(ip, _status)| ip.clone())
        .collect();
    let our_ip = network_cfg.routable_ip.ok_or(InternalError {
        message: "No own ip".into(),
    })?;

    let graph = interface
        .get_block_graph_status()
        .await
        .map_err(|_| warp::reject::reject())?;
    let finals = graph
        .latest_final_blocks_periods
        .iter()
        .enumerate()
        .map(|(i, (hash, period))| {
            (
                hash,
                *period,
                i as u8,
                get_block_slot_timestamp(
                    consensus_cfg.thread_count,
                    consensus_cfg.t0,
                    consensus_cfg.genesis_timestamp,
                    (*period, i as u8),
                )
                .unwrap(), // in a closure
            )
        })
        .collect::<Vec<(&Hash, u64, u8, UTime)>>();

    Ok(warp::reply::json(&json!({
        "time": time,
        "our_ip": our_ip,
        "last_final": finals,
        "nb_cliques": graph.max_cliques.len(),
        "nb_peers": connected_peers.len(),
    })))
}

#[cfg(test)]
mod test;
