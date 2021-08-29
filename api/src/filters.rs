// Copyright (c) 2021 MASSA LABS <info@massa.net>.

use super::config::ApiConfig;
use crate::ApiError;
use communication::network::Peer;
use communication::network::Peers;
use communication::{network::NetworkConfig, protocol::ProtocolConfig};
use consensus::ConsensusStats;
use consensus::ExportBlockStatus;
use consensus::Status;
use consensus::{
    get_block_slot_timestamp, get_latest_block_slot_at_timestamp, AddressState, BlockGraphExport,
    ConsensusConfig, ConsensusError, DiscardReason,
};
use crypto::signature::{PrivateKey, PublicKey, Signature};
use logging::massa_trace;
use models::Address;
use models::ModelsError;
use models::Operation;
use models::OperationId;
use models::OperationSearchResult;
use models::StakersCycleProductionStats;
use models::{BlockHeader, BlockId, Slot, Version};
use pool::PoolConfig;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    net::IpAddr,
};
use storage::StorageAccess;
use time::UTime;
use tokio::sync::{mpsc, oneshot};
use warp::{filters::BoxedFilter, Filter, Rejection, Reply};

/// Events that are transmitted outside the API
#[derive(Debug)]
pub enum ApiEvent {
    /// API received stop signal and wants to forward it
    AskStop,
    GetBlockGraphStatus(oneshot::Sender<BlockGraphExport>),
    GetBlockStatus {
        block_id: BlockId,
        response_tx: oneshot::Sender<Option<ExportBlockStatus>>,
    },
    GetPeers(oneshot::Sender<Peers>),
    GetSelectionDraw {
        start: Slot,
        end: Slot,
        response_tx: oneshot::Sender<Result<Vec<(Slot, (Address, Vec<Address>))>, ConsensusError>>,
    },
    AddOperations(HashMap<OperationId, Operation>),
    GetAddressesInfo {
        addresses: HashSet<Address>,
        response_tx: oneshot::Sender<Result<HashMap<Address, AddressState>, ConsensusError>>,
    },
    GetRecentOperations {
        address: Address,
        response_tx: oneshot::Sender<HashMap<OperationId, OperationSearchResult>>,
    },
    GetOperations {
        operation_ids: HashSet<OperationId>,
        /// if op was found: (operation, if it is in pool, map (blocks containing op and if they are final))
        response_tx: oneshot::Sender<HashMap<OperationId, OperationSearchResult>>,
    },
    GetStats(oneshot::Sender<ConsensusStats>),
    GetActiveStakers(oneshot::Sender<Option<HashMap<Address, u64>>>),
    RegisterStakingPrivateKeys(Vec<PrivateKey>),
    RemoveStakingAddresses(HashSet<Address>),
    GetStakingAddresses(oneshot::Sender<HashSet<Address>>),
    NodeSignMessage {
        message: Vec<u8>,
        response_tx: oneshot::Sender<(PublicKey, Signature)>,
    },
    GetStakersProductionStats {
        addrs: HashSet<Address>,
        response_tx: oneshot::Sender<Vec<StakersCycleProductionStats>>,
    },
    Unban(IpAddr),
    GetBlockIdsByCreator {
        address: Address,
        response_tx: oneshot::Sender<HashMap<BlockId, Status>>,
    },
}

pub enum ApiManagementCommand {}

#[derive(Debug, Deserialize, Clone, Copy)]
struct TimeInterval {
    start: Option<UTime>,
    end: Option<UTime>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OperationIds {
    pub operation_ids: HashSet<OperationId>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PrivateKeys {
    pub keys: Vec<PrivateKey>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Addresses {
    pub addrs: HashSet<Address>,
}
/// This function sets up all the routes that can be used
/// and combines them into one filter
///
pub fn get_filter(
    node_version: Version,
    api_config: ApiConfig,
    consensus_config: ConsensusConfig,
    _protocol_config: ProtocolConfig,
    network_config: NetworkConfig,
    pool_config: PoolConfig,
    event_tx: mpsc::Sender<ApiEvent>,
    opt_storage_command_sender: Option<StorageAccess>,
    clock_compensation: i64,
) -> BoxedFilter<(impl Reply,)> {
    massa_trace!("api.filters.get_filter", {});
    let evt_tx = event_tx.clone();
    let storage = opt_storage_command_sender.clone();
    let block = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("block"))
        .and(warp::path::param::<BlockId>()) //block id
        .and(warp::path::end())
        .and_then(move |hash| get_block(evt_tx.clone(), hash, storage.clone()));

    let evt_tx = event_tx.clone();
    let operations = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("get_operations"))
        .and(warp::path::end())
        .and(serde_qs::warp::query(serde_qs::Config::default()))
        .and_then(move |OperationIds { operation_ids }| {
            get_operations(evt_tx.clone(), operation_ids)
        });

    let evt_tx = event_tx.clone();
    let consensus_cfg = consensus_config.clone();

    let storage = opt_storage_command_sender.clone();
    let blockinterval = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("blockinterval"))
        .and(warp::path::end())
        .and(warp::query::<TimeInterval>()) //start, end
        .and_then(move |TimeInterval { start, end }| {
            get_block_interval(
                evt_tx.clone(),
                consensus_cfg.clone(),
                start,
                end,
                storage.clone(),
            )
        });

    let evt_tx = event_tx.clone();
    let current_parents = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("current_parents"))
        .and(warp::path::end())
        .and_then(move || get_current_parents(evt_tx.clone()));

    let evt_tx = event_tx.clone();
    let last_final = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_final"))
        .and(warp::path::end())
        .and_then(move || get_last_final(evt_tx.clone()));

    let evt_tx = event_tx.clone();
    let consensus_cfg = consensus_config.clone();
    let storage = opt_storage_command_sender.clone();
    let graph_interval = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("graph_interval"))
        .and(warp::path::end())
        .and(warp::query::<TimeInterval>()) //start, end //end
        .and_then(move |TimeInterval { start, end }| {
            get_graph_interval(
                evt_tx.clone(),
                consensus_cfg.clone(),
                start,
                end,
                storage.clone(),
            )
        });

    let evt_tx = event_tx.clone();
    let storage = opt_storage_command_sender;
    let block_ids_by_creator = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("block_ids_by_creator"))
        .and(warp::path::param::<Address>())
        .and(warp::path::end())
        .and_then(move |addr| get_block_ids_by_creator(evt_tx.clone(), addr, storage.clone()));

    let evt_tx = event_tx.clone();
    let cliques = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("cliques"))
        .and(warp::path::end())
        .and_then(move || get_cliques(evt_tx.clone()));

    let evt_tx = event_tx.clone();
    let peers = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("peers"))
        .and(warp::path::end())
        .and_then(move || get_peers(evt_tx.clone()));

    let network_cfg = network_config.clone();
    let our_ip = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("our_ip"))
        .and_then(move || get_our_ip(network_cfg.clone()));

    let evt_tx = event_tx.clone();
    let network_cfg = network_config.clone();
    let network_info = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("network_info"))
        .and(warp::path::end())
        .and_then(move || get_network_info(network_cfg.clone(), evt_tx.clone()));

    let node_config = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("node_config"))
        .and(warp::path::end())
        .and_then(get_node_config);

    let version = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("version"))
        .and(warp::path::end())
        .and_then(move || get_version(node_version));

    let pool_cfg = pool_config;
    let pool_config = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("pool_config"))
        .and(warp::path::end())
        .and_then(move || get_pool_config(pool_cfg.clone()));

    let consensus_cfg = consensus_config.clone();
    let get_consensus_cfg = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("consensus_config"))
        .and(warp::path::end())
        .and_then(move || get_consensus_config(consensus_cfg.clone()));

    let evt_tx = event_tx.clone();
    let network_cfg = network_config;
    let consensus_cfg = consensus_config.clone();
    let state = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("state"))
        .and(warp::path::end())
        .and_then(move || {
            get_state(
                evt_tx.clone(),
                consensus_cfg.clone(),
                network_cfg.clone(),
                clock_compensation,
            )
        });

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
        .and(warp::path::param::<Address>())
        .and(warp::path::end())
        .and_then(move |creator| {
            get_staker_info(
                evt_tx.clone(),
                api_cfg.clone(),
                consensus_cfg.clone(),
                creator,
                clock_compensation,
            )
        });

    let evt_tx = event_tx.clone();
    let staker_stats = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("staker_stats"))
        .and(warp::path::end())
        .and(serde_qs::warp::query(serde_qs::Config::default()))
        .and_then(move |Addresses { addrs }| get_stakers_stats(evt_tx.clone(), addrs));

    let evt_tx = event_tx.clone();
    let api_cfg = api_config;
    let consensus_cfg = consensus_config;
    let next_draws = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("next_draws"))
        .and(warp::path::end())
        .and(serde_qs::warp::query(serde_qs::Config::default()))
        .and_then(move |Addresses { addrs }| {
            get_next_draws(
                evt_tx.clone(),
                api_cfg.clone(),
                consensus_cfg.clone(),
                addrs,
                clock_compensation,
            )
        });

    let evt_tx = event_tx.clone();
    let operations_involving_address = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("operations_involving_address"))
        .and(warp::path::param::<Address>())
        .and(warp::path::end())
        .and_then(move |address| get_operations_involving_address(evt_tx.clone(), address));

    let evt_tx = event_tx.clone();
    let addresses_info = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("addresses_info"))
        .and(warp::path::end())
        .and(serde_qs::warp::query(serde_qs::Config::default()))
        .and_then(move |Addresses { addrs }| get_addresses_info(addrs, evt_tx.clone()));

    let evt_tx = event_tx.clone();
    let stop_node = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("stop_node"))
        .and(warp::path::end())
        .and_then(move || stop_node(evt_tx.clone()));

    let evt_tx = event_tx.clone();
    let unban = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("unban"))
        .and(warp::path::param::<IpAddr>())
        .and(warp::path::end())
        .and_then(move |ip| unban(evt_tx.clone(), ip));

    let evt_tx = event_tx.clone();
    let register_staking_private_keys = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("register_staking_keys"))
        .and(warp::path::end())
        .and(serde_qs::warp::query(serde_qs::Config::default()))
        .and_then(move |PrivateKeys { keys }| register_staking_private_keys(evt_tx.clone(), keys));

    let evt_tx = event_tx.clone();
    let remove_staking_addresses = warp::delete()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("remove_staking_addresses"))
        .and(warp::path::end())
        .and(serde_qs::warp::query(serde_qs::Config::default()))
        .and_then(move |Addresses { addrs }| remove_staking_addresses(evt_tx.clone(), addrs));

    let evt_tx = event_tx.clone();
    let send_operations = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("send_operations"))
        .and(warp::path::end())
        .and(warp::body::json())
        .and_then(move |operations| send_operations(operations, evt_tx.clone()));

    let evt_tx = event_tx.clone();
    let get_stats = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("get_stats"))
        .and(warp::path::end())
        .and_then(move || get_stats(evt_tx.clone()));

    let evt_tx = event_tx.clone();
    let get_active_stakers = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("active_stakers"))
        .and(warp::path::end())
        .and_then(move || get_active_stakers(evt_tx.clone()));

    let evt_tx = event_tx.clone();
    let staking_addresses = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("staking_addresses"))
        .and(warp::path::end())
        .and_then(move || get_staking_addresses(evt_tx.clone()));

    let evt_tx = event_tx;
    let node_sign_message = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("node_sign_message"))
        .and(warp::path::end())
        .and(warp::body::bytes())
        .and_then(move |msg: warp::hyper::body::Bytes| node_sign_msg(msg.to_vec(), evt_tx.clone()));

    block
        .or(blockinterval)
        .or(current_parents)
        .or(last_final)
        .or(graph_interval)
        .or(cliques)
        .or(peers)
        .or(unban)
        .or(our_ip)
        .or(network_info)
        .or(state)
        .or(last_stale)
        .or(last_invalid)
        .or(staker_info)
        .or(staker_stats)
        .or(addresses_info)
        .or(stop_node)
        .or(send_operations)
        .or(node_config)
        .or(pool_config)
        .or(get_consensus_cfg)
        .or(operations_involving_address)
        .or(operations)
        .or(next_draws)
        .or(get_stats)
        .or(get_active_stakers)
        .or(staking_addresses)
        .or(register_staking_private_keys)
        .or(remove_staking_addresses)
        .or(node_sign_message)
        .or(block_ids_by_creator)
        .or(version)
        .boxed()
}

async fn get_pool_config(config: PoolConfig) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_pool_config", {});
    Ok(warp::reply::json(&json!({
        "max_pool_size_per_thread": config.max_pool_size_per_thread,
        "max_operation_future_validity_start_periods": config.max_operation_future_validity_start_periods,
    })))
}

/// Returns our ip address
///
/// Note: as our ip address is in the config,
/// this function is more about getting every bit of
/// information we want exactly in the same way
async fn get_node_config() -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_node_config", {});
    let context = models::with_serialization_context(|context| context.clone());
    Ok(warp::reply::json(&context))
}

async fn get_version(version: Version) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_version", {});
    Ok(warp::reply::json(&version))
}

async fn get_consensus_config(
    consensus_cfg: ConsensusConfig,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_consensus_config", {});
    Ok(warp::reply::json(&json!({
        "t0": consensus_cfg.t0,
        "thread_count": consensus_cfg.thread_count,
        "genesis_timestamp": consensus_cfg.genesis_timestamp,
        "delta_f0": consensus_cfg.delta_f0,
        "max_block_size": consensus_cfg.max_block_size,
        "operation_validity_periods": consensus_cfg.operation_validity_periods,
        "periods_per_cycle": consensus_cfg.periods_per_cycle,
        "roll_price": consensus_cfg.roll_price,
    })))
}

/// This function sends AskStop outside the Api and
/// return the result as a warp reply.
///
/// # Argument
/// * event_tx : Sender used to send the event out
async fn stop_node(evt_tx: mpsc::Sender<ApiEvent>) -> Result<impl Reply, Rejection> {
    massa_trace!("api.filters.stop_node", {});
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

async fn unban(evt_tx: mpsc::Sender<ApiEvent>, ip: IpAddr) -> Result<impl Reply, Rejection> {
    massa_trace!("api.filters.unban", {});
    match evt_tx.send(ApiEvent::Unban(ip)).await {
        Ok(_) => Ok(warp::reply().into_response()),
        Err(err) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error unbanning : {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
    }
}

async fn register_staking_private_keys(
    evt_tx: mpsc::Sender<ApiEvent>,
    keys: Vec<PrivateKey>,
) -> Result<impl Reply, Rejection> {
    massa_trace!("api.filters.register_staking_private_keys", {});

    match evt_tx
        .send(ApiEvent::RegisterStakingPrivateKeys(keys))
        .await
    {
        Ok(_) => Ok(warp::reply().into_response()),
        Err(err) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error registering _staking_private_keys : {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
    }
}

async fn remove_staking_addresses(
    evt_tx: mpsc::Sender<ApiEvent>,
    addrs: HashSet<Address>,
) -> Result<impl Reply, Rejection> {
    massa_trace!("api.filters.remove_staking_addresses", {});
    match evt_tx.send(ApiEvent::RemoveStakingAddresses(addrs)).await {
        Ok(_) => Ok(warp::reply().into_response()),
        Err(err) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error removing _staking_addresses : {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
    }
}
/// This function sends the new transaction outside the Api and
/// return the result as a warp reply.
///
/// The transaction is verified before been propagated.
/// # Argument
/// * event_tx : Sender used to send the event out
async fn send_operations(
    operations: Vec<Operation>,
    evt_tx: mpsc::Sender<ApiEvent>,
) -> Result<impl Reply, Rejection> {
    massa_trace!("api.filters.send_operations ", { "operations": operations });
    let to_send: Result<HashMap<OperationId, Operation>, ModelsError> = operations
        .into_iter()
        .map(|op| Ok((op.verify_integrity()?, op)))
        .collect();
    let to_send = match to_send {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error during operation verification : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(to_send) => to_send,
    };

    let opid_list = to_send
        .iter()
        .map(|(opid, _)| *opid)
        .collect::<Vec<OperationId>>();

    match evt_tx.send(ApiEvent::AddOperations(to_send)).await {
        Ok(_) => Ok(warp::reply::json(&opid_list).into_response()),
        Err(err) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error during sending operation : {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
    }
}

/// Returns block with given hash as a reply
///
async fn get_block(
    event_tx: mpsc::Sender<ApiEvent>,
    block_id: BlockId,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<impl Reply, Rejection> {
    massa_trace!("api.filters.get_block", { "block_id": block_id });
    match retrieve_block(block_id, &event_tx).await {
        Err(err) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error retrieving active blocks : {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
        Ok(None) => {
            if let Some(cmd_tx) = opt_storage_command_sender {
                match cmd_tx.get_block(block_id).await {
                    Ok(Some(block)) => Ok(warp::reply::json(&block).into_response()),
                    Ok(None) => Ok(warp::reply::with_status(
                        warp::reply::json(&json!({
                            "message": format!("active block not found : {:?}", block_id)
                        })),
                        warp::http::StatusCode::NOT_FOUND,
                    )
                    .into_response()),
                    Err(e) => Ok(warp::reply::with_status(
                        warp::reply::json(&json!({
                            "message": format!("error retrieving active blocks : {:?}", e)
                        })),
                        warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                    )
                    .into_response()),
                }
            } else {
                Ok(warp::reply::with_status(
                    warp::reply::json(&json!({
                        "message": format!("active block not found : {:?}", block_id)
                    })),
                    warp::http::StatusCode::NOT_FOUND,
                )
                .into_response())
            }
        }
        Ok(Some(block)) => Ok(warp::reply::json(&block).into_response()),
    }
}

async fn get_block_ids_by_creator(
    event_tx: mpsc::Sender<ApiEvent>,
    address: Address,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<impl Reply, Rejection> {
    massa_trace!("api.filters.get_block_ids_by_creator", {
        "address": address
    });
    let res = match retrieve_block_ids_by_creator_from_consensus(address, &event_tx).await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving active blocks : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(mut res) => {
            if let Some(cmd_tx) = opt_storage_command_sender {
                match retrieve_block_ids_by_creator_from_storage(address, &cmd_tx).await {
                    Ok(res2) => res.extend(res2),
                    Err(e) => {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&json!({
                                "message": format!("error retrieving active blocks : {:?}", e)
                            })),
                            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                        )
                        .into_response())
                    }
                }
            }
            res
        }
    };
    Ok(warp::reply::json(&res).into_response())
}

async fn get_operations(
    event_tx: mpsc::Sender<ApiEvent>,
    operation_ids: HashSet<OperationId>,
) -> Result<impl Reply, Rejection> {
    massa_trace!("api.filters.get_operations", {
        "operation_ids": operation_ids
    });
    match retrieve_operations(operation_ids, &event_tx).await {
        Err(err) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error retrieving operation : {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
        Ok(ops) => Ok(warp::reply::json(&json!(ops
            .into_iter()
            .collect::<Vec<(OperationId, OperationSearchResult)>>()))
        .into_response()),
    }
}

async fn do_node_sign_msg(
    message: Vec<u8>,
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<(PublicKey, Signature), ApiError> {
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::NodeSignMessage {
            message,
            response_tx,
        })
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!(
                "Could not send api event node sign message : {0}",
                e
            ))
        })?;
    response_rx.await.map_err(|e| {
        ApiError::ReceiveChannelError(format!(
            "Could not retrieve node message signature : {0}",
            e
        ))
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PubkeySig {
    pub public_key: PublicKey,
    pub signature: Signature,
}
pub async fn node_sign_msg(
    msg: Vec<u8>,
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<impl Reply, Rejection> {
    massa_trace!("api.filters.node_sign_msg", { "msg": msg });
    match do_node_sign_msg(msg, &event_tx).await {
        Err(err) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error while making node sign message: {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
        Ok((public_key, signature)) => Ok(warp::reply::json(&json!(PubkeySig {
            public_key,
            signature
        }))
        .into_response()),
    }
}

/// Returns our ip address
///
/// Note: as our ip address is in the config,
/// this function is more about getting every bit of
/// information we want exactly in the same way
async fn get_our_ip(network_cfg: NetworkConfig) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_our_ip", {});
    Ok(warp::reply::json(&network_cfg.routable_ip))
}

async fn retrieve_graph_export(
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<BlockGraphExport, ApiError> {
    massa_trace!("api.filters.retrieve_graph_export", {});
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetBlockGraphStatus(response_tx))
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!("could not send api event get block graph : {0}", e))
        })?;
    response_rx.await.map_err(|e| {
        ApiError::ReceiveChannelError(format!("could not retrieve block graph: {0}", e))
    })
}

async fn retrieve_block(
    block_id: BlockId,
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<Option<ExportBlockStatus>, ApiError> {
    massa_trace!("api.filters.retrieve_block", { "block_id": block_id });
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetBlockStatus {
            block_id,
            response_tx,
        })
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!("Could not send api event get block : {0}", e))
        })?;
    response_rx
        .await
        .map_err(|e| ApiError::ReceiveChannelError(format!("Could not retrieve block : {0}", e)))
}

async fn retrieve_block_ids_by_creator_from_consensus(
    address: Address,
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<HashMap<BlockId, Status>, ApiError> {
    massa_trace!("api.filters.retrieve_block_ids_by_creator", {
        "address": address
    });
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetBlockIdsByCreator {
            address,
            response_tx,
        })
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!(
                "Could not send api event GetBlockIdsByCreator : {0}",
                e
            ))
        })?;
    response_rx.await.map_err(|e| {
        ApiError::ReceiveChannelError(format!("Could not retrieve GetBlockIdsByCreator : {0}", e))
    })
}

async fn retrieve_block_ids_by_creator_from_storage(
    address: Address,
    storage_access: &StorageAccess,
) -> Result<HashMap<BlockId, Status>, ApiError> {
    Ok(storage_access
        .get_block_ids_by_creator(&address)
        .await?
        .into_iter()
        .map(|id| (id, Status::Final))
        .collect())
}

async fn retrieve_operations(
    operation_ids: HashSet<OperationId>,
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<HashMap<OperationId, OperationSearchResult>, ApiError> {
    massa_trace!("api.filters.retrieve_operations", {
        "operation_ids": operation_ids
    });
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetOperations {
            operation_ids,
            response_tx,
        })
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!("Could not send api event get operation : {0}", e))
        })?;
    response_rx.await.map_err(|e| {
        ApiError::ReceiveChannelError(format!("Could not retrieve operation : {0}", e))
    })
}

async fn retrieve_peers_and_nodeid(event_tx: &mpsc::Sender<ApiEvent>) -> Result<Peers, ApiError> {
    massa_trace!("api.filters.retrieve_peers", {});
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetPeers(response_tx))
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!("Could not send api event get peers : {0}", e))
        })?;
    response_rx.await.map_err(|e| {
        ApiError::ReceiveChannelError(format!("Could not retrieve block peers: {0}", e))
    })
}

async fn retrieve_selection_draw(
    start: Slot,
    end: Slot,
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<Vec<(Slot, (Address, Vec<Address>))>, ApiError> {
    massa_trace!("api.filters.retrieve_selection_draw", {});
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetSelectionDraw {
            start,
            end,
            response_tx,
        })
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

/// Returns best parents wrapped in a reply.
/// The Slot represents the parent's slot.
///
async fn get_current_parents(
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_current_parents", {});
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

    let parents = graph.best_parents;
    let mut best = Vec::new();
    for (hash, _) in parents {
        match graph.active_blocks.get_key_value(&hash) {
            Some((_, block)) => best.push((hash, block.block.content.slot)),
            None => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&json!({
                        "message":
                            "inconsistency error between best_parents and active_blocks"
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response())
            }
        }
    }

    Ok(warp::reply::json(&best).into_response())
}

/// Returns last final blocks wrapped in a reply.
///
async fn get_last_final(
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_last_final", {});
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
    let finals = graph
        .latest_final_blocks_periods
        .iter()
        .enumerate()
        .map(|(i, (hash, period))| (hash, Slot::new(*period, i as u8)))
        .collect::<Vec<(&BlockId, Slot)>>();
    Ok(warp::reply::json(&finals).into_response())
}

/// Returns all blocks in a time interval wrapped in a reply.
///
/// Note: both start time is included and end time is excluded
async fn get_block_interval(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: ConsensusConfig,
    start: Option<UTime>,
    end: Option<UTime>,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_block_interval", {});
    match get_block_interval_process(
        event_tx,
        consensus_cfg,
        start,
        end,
        opt_storage_command_sender,
    )
    .await
    {
        Ok(map) => Ok(warp::reply::json(&map).into_response()),
        Err(err) => {
            println!("get_block_interval error:{:?}", err);
            Ok(warp::reply::with_status(
                warp::reply::json(&json!({ "message": err })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    }
}

async fn get_block_from_graph(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: &ConsensusConfig,
    start_opt: Option<UTime>,
    end_opt: Option<UTime>,
) -> Result<Vec<(BlockId, Slot)>, String> {
    massa_trace!("api.filters.get_block_from_graph", {});
    retrieve_graph_export(&event_tx)
        .await
        .map_err(|err| (format!("error retrieving graph : {:?}", err)))
        .and_then(|graph| {
            let start = start_opt.unwrap_or_else(|| UTime::from(0));
            let end = end_opt.unwrap_or_else(|| UTime::from(u64::MAX));

            graph
                .active_blocks
                .into_iter()
                .filter_map(|(hash, exported_block)| {
                    get_block_slot_timestamp(
                        consensus_cfg.thread_count,
                        consensus_cfg.t0,
                        consensus_cfg.genesis_timestamp,
                        exported_block.block.content.slot,
                    )
                    .map_err(|err| (format!("error getting time : {:?}", err)))
                    .map(|time| {
                        if start <= time && time < end {
                            Some((hash, exported_block.block.content.slot))
                        } else {
                            None
                        }
                    })
                    .transpose()
                })
                .collect::<Result<Vec<(BlockId, Slot)>, String>>()
        })
}

async fn get_block_interval_process(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: ConsensusConfig,
    start_opt: Option<UTime>,
    end_opt: Option<UTime>,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<Vec<(BlockId, Slot)>, String> {
    massa_trace!("api.filters.get_block_interval_process", {});
    if start_opt
        .and_then(|s| end_opt.and_then(|e| if s >= e { Some(()) } else { None }))
        .is_some()
    {
        return Ok(vec![]);
    }

    //filter block from graph_export
    let mut res = get_block_from_graph(event_tx, &consensus_cfg, start_opt, end_opt).await?;

    if let Some(ref storage) = opt_storage_command_sender {
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
                    .unwrap_or_else(|| Ok(Slot::new(0, 0)))
            })?;
            Some(slot)
        } else {
            None
        };

        //get end slot
        let end_slot_opt = if let Some(end) = end_opt {
            get_latest_block_slot_at_timestamp(
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
            })?
        } else {
            None
        };

        storage
            .get_slot_range(start_slot, end_slot_opt)
            .await
            .map(|blocks| {
                res.append(
                    &mut blocks
                        .into_iter()
                        .map(|(hash, block)| (hash, block.header.content.slot))
                        .collect(),
                )
            })
            .map_err(|err| (format!("error retrieving slot range: {:?}", err)))?;
    }

    Ok(res)

    //old version
    /*    let graph = match retrieve_graph_export(&event_tx).await {
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
            header.slot,
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
        if start <= time && time < end {
            res.insert(hash, header.slot);
        }
    }
    if let Some(storage) = opt_storage_command_sender {
        let start_slot = match get_latest_block_slot_at_timestamp(
            consensus_cfg.thread_count,
            consensus_cfg.t0,
            consensus_cfg.genesis_timestamp,
            start,
        ) {
            Err(err) => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&json!({
                        "message": format!("error retrieving slot from time: {:?}", err)
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response())
            }
            Ok(Some(slot)) => {
                if match get_block_slot_timestamp(
                    consensus_cfg.thread_count,
                    consensus_cfg.t0,
                    consensus_cfg.genesis_timestamp,
                    slot,
                ) {
                    Ok(start_time) => start_time,
                    Err(e) => {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&json!({
                                "message": format!("error retrieving next slot: {:?}", e)
                            })),
                            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                        )
                        .into_response())
                    }
                } == start
                {
                    slot
                } else {
                    match slot.get_next_slot(consensus_cfg.thread_count) {
                        Ok(next) => next,
                        Err(e) => {
                            return Ok(warp::reply::with_status(
                                warp::reply::json(&json!({
                                    "message": format!("error retrieving next slot: {:?}", e)
                                })),
                                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                            )
                            .into_response())
                        }
                    }
                }
            }
            Ok(None) => Slot::new(0, 0),
        };
        let end_slot = match get_latest_block_slot_at_timestamp(
            consensus_cfg.thread_count,
            consensus_cfg.t0,
            consensus_cfg.genesis_timestamp,
            end,
        ) {
            Err(err) => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&json!({
                        "message": format!("error retrieving slot from time: {:?}", err)
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response())
            }
            Ok(Some(slot)) => {
                if match get_block_slot_timestamp(
                    consensus_cfg.thread_count,
                    consensus_cfg.t0,
                    consensus_cfg.genesis_timestamp,
                    slot,
                ) {
                    Ok(end_time) => end_time,
                    Err(e) => {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&json!({
                                "message": format!("error retrieving next slot: {:?}", e)
                            })),
                            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                        )
                        .into_response())
                    }
                } == end
                {
                    slot
                } else {
                    match slot.get_next_slot(consensus_cfg.thread_count) {
                        Ok(next) => next,
                        Err(e) => {
                            return Ok(warp::reply::with_status(
                                warp::reply::json(&json!({
                                    "message": format!("error retrieving next slot: {:?}", e)
                                })),
                                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                            )
                            .into_response())
                        }
                    }
                }
            }
            Ok(None) => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&json!({
                        "message": format!("error retrieving end slot from time : no slot found")
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response())
            }
        };

        let blocks = match storage
            .get_slot_range(Some(start_slot), Some(end_slot))
            .await
        {
            Err(err) => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&json!({
                        "message": format!("error retrieving slot range: {:?}", err)
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )
                .into_response())
            }
            Ok(blocks) => blocks,
        };
        for (hash, block) in blocks {
            res.insert(hash, block.header.slot);
        }
    }

    Ok(res)*/
}

/// Returns all block info needed to reconstruct the graph found in the time interval.
/// The result is a vec of (hash, period, thread, status, parents hash) wrapped in a reply.
///
/// Note:
/// * both start time is included and end time is excluded
/// * status is in ["active", "final", "stale"]
async fn get_graph_interval(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: ConsensusConfig,
    start_opt: Option<UTime>,
    end_opt: Option<UTime>,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_graph_interval", {});
    match get_graph_interval_process(
        event_tx,
        consensus_cfg,
        start_opt,
        end_opt,
        opt_storage_command_sender,
    )
    .await
    {
        Ok(map) => Ok(warp::reply::json(&map).into_response()),
        Err(err) => Ok(warp::reply::with_status(
            warp::reply::json(&json!({ "message": err })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response()),
    }
}

async fn get_graph_interval_process(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: ConsensusConfig,
    start_opt: Option<UTime>,
    end_opt: Option<UTime>,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<Vec<(BlockId, Slot, Status, Vec<BlockId>)>, String> {
    massa_trace!("api.filters.get_graph_interval_process", {});
    //filter block from graph_export
    let mut res = retrieve_graph_export(&event_tx)
        .await
        .map_err(|err| (format!("error retrieving graph : {:?}", err)))
        .and_then(|graph| {
            let start = start_opt.unwrap_or_else(|| UTime::from(0));
            let end = end_opt.unwrap_or_else(|| UTime::from(u64::MAX));

            graph
                .active_blocks
                .into_iter()
                .filter_map(|(hash, exported_block)| {
                    let header = exported_block.block;
                    let status = exported_block.status;
                    get_block_slot_timestamp(
                        consensus_cfg.thread_count,
                        consensus_cfg.t0,
                        consensus_cfg.genesis_timestamp,
                        header.content.slot,
                    )
                    .map_err(|err| (format!("error getting time : {:?}", err)))
                    .map(|time| {
                        if start <= time && time < end {
                            Some((hash, (header.content.slot, status, header.content.parents)))
                        } else {
                            None
                        }
                    })
                    .transpose()
                })
                .collect::<Result<Vec<(BlockId, (Slot, Status, Vec<BlockId>))>, String>>()
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
            res.push((
                hash,
                (
                    block.header.content.slot,
                    Status::Final,
                    block.header.content.parents.clone(),
                ),
            ));
        }
    }
    Ok(res
        .into_iter()
        .map(|(hash, (slot, status, parents))| (hash, slot, status, parents))
        .collect())
}

/// Returns number of cliques and current cliques as Vec<HashSet<(hash, (period, thread))>>
/// The result is a tuple (number_of_cliques, current_cliques) wrapped in a reply.
///
async fn get_cliques(
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_cliques", {});
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

    let mut hashes = HashSet::new();
    for clique in graph.max_cliques.iter() {
        hashes.extend(&clique.block_ids)
    }

    let mut hashes_map = HashMap::new();
    for hash in hashes.iter() {
        if let Some((_, block)) = graph.active_blocks.get_key_value(hash) {
            hashes_map.insert(hash, block.block.content.slot);
        } else {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": "inconstancy error between cliques and active_blocks"
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response());
        }
    }

    let mut res = Vec::new();
    for clique in graph.max_cliques.iter() {
        let mut set = HashSet::new();
        for hash in clique.block_ids.iter() {
            match hashes_map.get_key_value(hash) {
                Some((k, v)) => {
                    set.insert((k, *v));
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
    network_cfg: NetworkConfig,
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_network_info", {});
    let (peers, node_id) = match retrieve_peers_and_nodeid(&event_tx).await {
        Ok(Peers { our_node_id, peers }) => (peers, our_node_id),
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
        "node_id": node_id,
    }))
    .into_response())
}

/// Returns state info for a set of addresses
///
async fn get_addresses_info(
    addresses: HashSet<Address>,
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_addresses_info", { "addresses": addresses });
    let (response_tx, response_rx) = oneshot::channel();
    if let Err(err) = event_tx
        .send(ApiEvent::GetAddressesInfo {
            addresses,
            response_tx,
        })
        .await
    {
        return Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error during sending ledger data : {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response());
    }

    let addrs_info = match response_rx.await {
        Ok(Ok(addrs_info)) => addrs_info,
        Ok(Err(err)) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error during exporting addrs info: {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error get addrs info: {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    };

    Ok(warp::reply::json(&addrs_info).into_response())
}

/// Returns connected peers :
/// - ip address
/// - peer info (see PeerInfo struct in communication::network::PeerInfoDatabase)
///
async fn get_peers(event_tx: mpsc::Sender<ApiEvent>) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_peers", {});
    let peers = match retrieve_peers_and_nodeid(&event_tx).await {
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

async fn get_operations_involving_address(
    event_tx: mpsc::Sender<ApiEvent>,
    address: Address,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_operations_involving_address", {
        "address": address
    });
    let (response_tx, response_rx) = oneshot::channel();
    if let Err(err) = event_tx
        .send(ApiEvent::GetRecentOperations {
            address,
            response_tx,
        })
        .await
    {
        return Ok(warp::reply::with_status(
            warp::reply::json(&json!({
                "message": format!("error during sending ledger data : {:?}", err)
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response());
    }

    let res = match response_rx.await {
        Ok(res) => res,

        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error get ledger data : {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    };

    Ok(warp::reply::json(&res).into_response())
}

/// Returns a summary of the current state:
/// * time in UTime
/// * latest slot (optional)
/// * last final block
/// * number of cliques
/// * number of connected peers
///
async fn get_state(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: ConsensusConfig,
    network_cfg: NetworkConfig,
    clock_compensation: i64,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_state", {});
    let cur_time = match UTime::now(clock_compensation) {
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

    let peers = match retrieve_peers_and_nodeid(&event_tx).await {
        Ok(peers) => peers.peers,
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
        .filter(|(_ip, Peer { peer_info, .. })| {
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
        .collect::<Result<Vec<(&BlockId, Slot, UTime)>, consensus::ConsensusError>>()
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
        "current_cycle": latest_slot_opt.unwrap_or_else(|| Slot::new(0,0)).get_cycle(consensus_cfg.periods_per_cycle),
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
    massa_trace!("api.filters.get_last_stale", {});
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

    let discarded = graph.discarded_blocks;
    let mut discarded = discarded
        .map
        .iter()
        .filter(|(_hash, (reason, _header))| *reason == DiscardReason::Stale)
        .map(|(hash, (_reason, header))| (hash, header.content.slot))
        .collect::<Vec<(&BlockId, Slot)>>();
    if !discarded.is_empty() {
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
    massa_trace!("api.filters.get_last_invalid", {});
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
    let discarded = graph.discarded_blocks;

    let mut discarded = discarded
        .map
        .iter()
        .filter(|(_hash, (reason, _header))| matches!(reason, DiscardReason::Invalid(_)))
        .map(|(hash, (_reason, header))| (hash, header.content.slot))
        .collect::<Vec<(&BlockId, Slot)>>();
    if !discarded.is_empty() {
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
    creator: Address,
    clock_compensation: i64,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_staker_info", {});
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
        .filter(|(_hash, block)| {
            Address::from_public_key(&block.block.content.creator).unwrap() == creator
        })
        .map(|(hash, block)| (hash, block.block.clone()))
        .collect::<Vec<(&BlockId, BlockHeader)>>();

    let discarded = graph
        .discarded_blocks
        .map
        .iter()
        .filter(|(_hash, (_reason, header))| {
            Address::from_public_key(&header.content.creator).unwrap() == creator
        })
        .map(|(hash, (reason, header))| (hash, reason.clone(), header.clone()))
        .collect::<Vec<(&BlockId, DiscardReason, BlockHeader)>>();
    let cur_time = match UTime::now(clock_compensation) {
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
    .unwrap_or_else(|| Slot::new(0, 0));
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
        .filter_map(|(slt, (sel, _))| {
            // todo retrieve next endorsment by staker ?
            if sel == creator {
                return Some(slt);
            }
            None
        })
        .collect();

    Ok(warp::reply::json(&json!({
        "staker_active_blocks": blocks,
        "staker_discarded_blocks": discarded,
        "staker_next_draws": next_slots_by_creator
    }))
    .into_response())
}

/// Returns staker production stats
async fn get_stakers_stats(
    event_tx: mpsc::Sender<ApiEvent>,
    addrs: HashSet<Address>,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_stakers_stats", { "addrs": addrs });

    let stakers_production_stats = match retrieve_stakers_production_stats(addrs, &event_tx).await {
        Ok(stats) => stats,
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error getting staker production stats: {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
    };

    Ok(warp::reply::json(&json!(stakers_production_stats)).into_response())
}

async fn get_next_draws(
    event_tx: mpsc::Sender<ApiEvent>,
    api_cfg: ApiConfig,
    consensus_cfg: ConsensusConfig,
    addresses: HashSet<Address>,
    clock_compensation: i64,
) -> Result<impl warp::Reply, warp::Rejection> {
    let cur_time = match UTime::now(clock_compensation) {
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
    .unwrap_or_else(|| Slot::new(0, 0));
    let end_slot = Slot::new(
        start_slot
            .period
            .saturating_add(api_cfg.selection_return_periods),
        start_slot.thread,
    );

    let next_slots: Vec<(Address, Slot)> =
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
        .filter_map(|(slt, (sel, _))| {
            if addresses.contains(&sel) {
                // todo retrive endorsements by addressess ?
                return Some((sel, slt));
            }
            None
        })
        .collect();

    Ok(warp::reply::json(&next_slots).into_response())
}

async fn retrieve_stakers_production_stats(
    addrs: HashSet<Address>,
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<Vec<StakersCycleProductionStats>, ApiError> {
    massa_trace!("api.filters.retrieve_stakers_production_stats", {
        "addrs": addrs
    });
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetStakersProductionStats { addrs, response_tx })
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!(
                "Could not send api event get staker production stats: {0}",
                e
            ))
        })?;
    response_rx.await.map_err(|e| {
        ApiError::ReceiveChannelError(format!(
            "Could not retrieve stakers produciton stats: {0}",
            e
        ))
    })
}

async fn retrieve_stats(event_tx: &mpsc::Sender<ApiEvent>) -> Result<ConsensusStats, ApiError> {
    massa_trace!("api.filters.retrieve_stats", {});
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetStats(response_tx))
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!("Could not send api event get stats: {0}", e))
        })?;
    response_rx
        .await
        .map_err(|e| ApiError::ReceiveChannelError(format!("Could not retrieve stats: {0}", e)))
}

async fn get_stats(event_tx: mpsc::Sender<ApiEvent>) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_stats", {});
    let stats = match retrieve_stats(&event_tx).await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving stats: {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(stats) => stats,
    };
    Ok(warp::reply::json(&json!(stats)).into_response())
}

async fn retrieve_staking_addresses(
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<HashSet<Address>, ApiError> {
    massa_trace!("api.filters.retrieve_staking_addresses", {});
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetStakingAddresses(response_tx))
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!(
                "Could not send api event get_staking_addresses: {0}",
                e
            ))
        })?;
    response_rx
        .await
        .map_err(|e| ApiError::ReceiveChannelError(format!("Could not retrieve stats: {0}", e)))
}

async fn get_staking_addresses(
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_staking_addresses", {});
    let addresses = match retrieve_staking_addresses(&event_tx).await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving staking_addresses: {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(addresses) => addresses,
    };
    Ok(warp::reply::json(&json!(addresses)).into_response())
}

async fn retrieve_active_stakers(
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<Option<HashMap<Address, u64>>, ApiError> {
    massa_trace!("api.filters.retrieve_active_stakers", {});
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetActiveStakers(response_tx))
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!(
                "Could not send api event get active stakers: {0}",
                e
            ))
        })?;
    response_rx.await.map_err(|e| {
        ApiError::ReceiveChannelError(format!("Could not retrieve active stakers: {0}", e))
    })
}

async fn get_active_stakers(
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<impl warp::Reply, warp::Rejection> {
    massa_trace!("api.filters.get_active_stakers", {});
    let stats = match retrieve_active_stakers(&event_tx).await {
        Err(err) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": format!("error retrieving active stakers: {:?}", err)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response())
        }
        Ok(None) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&json!({
                    "message": "active staking cycle not available/complete"
                })),
                warp::http::StatusCode::NOT_FOUND,
            )
            .into_response())
        }
        Ok(Some(stats)) => stats,
    };
    Ok(warp::reply::json(&json!(stats)).into_response())
}
