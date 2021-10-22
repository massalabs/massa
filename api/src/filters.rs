// Copyright (c) 2021 MASSA LABS <info@massa.net>.

use std::{
    cmp::min,
    collections::{HashMap, HashSet},
    net::IpAddr,
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use warp::{filters::BoxedFilter, Filter, Rejection, Reply};

use consensus::error::ConsensusError;
use consensus::ExportBlockStatus;
use consensus::Status;
use consensus::{BlockGraphExport, ConsensusConfig, DiscardReason};
use crypto::signature::PrivateKey;
use logging::massa_trace;
use models::api::TimeInterval;
use models::node::NodeId;
use models::stats::ConsensusStats;
use models::Address;
use models::Amount;
use models::Operation;
use models::OperationId;
use models::OperationSearchResult;
use models::SerializationContext;
use models::StakersCycleProductionStats;
use models::{
    address::{AddressHashMap, AddressHashSet, AddressState, Addresses},
    BlockHashMap, BlockHashSet, OperationHashMap, OperationHashSet,
};
use models::{
    crypto::PubkeySig,
    timeslots::{
        get_block_slot_timestamp, get_latest_block_slot_at_timestamp, time_range_to_slot_range,
    },
};
use models::{BlockHeader, BlockId, Slot, Version};
use network::NetworkConfig;
use network::Peer;
use network::Peers;
use pool::PoolConfig;
use protocol::ProtocolConfig;
use storage::StorageAccess;
use time::UTime;

use crate::ApiError;

use super::config::ApiConfig;

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
    AddOperations(OperationHashMap<Operation>),
    GetAddressesInfo {
        addresses: AddressHashSet,
        response_tx: oneshot::Sender<Result<AddressHashMap<AddressState>, ConsensusError>>,
    },
    GetRecentOperations {
        address: Address,
        response_tx: oneshot::Sender<OperationHashMap<OperationSearchResult>>,
    },
    GetOperations {
        operation_ids: OperationHashSet,
        /// if op was found: (operation, if it is in pool, map (blocks containing op and if they are final))
        response_tx: oneshot::Sender<OperationHashMap<OperationSearchResult>>,
    },
    GetStats(oneshot::Sender<ConsensusStats>),
    GetActiveStakers(oneshot::Sender<Option<AddressHashMap<u64>>>),
    RegisterStakingPrivateKeys(Vec<PrivateKey>),
    RemoveStakingAddresses(AddressHashSet),
    GetStakingAddresses(oneshot::Sender<AddressHashSet>),
    NodeSignMessage {
        message: Vec<u8>,
        response_tx: oneshot::Sender<PubkeySig>,
    },
    GetStakersProductionStats {
        addrs: AddressHashSet,
        response_tx: oneshot::Sender<Vec<StakersCycleProductionStats>>,
    },
    Unban(IpAddr),
    GetBlockIdsByCreator {
        address: Address,
        response_tx: oneshot::Sender<BlockHashMap<Status>>,
    },
}

pub enum ApiManagementCommand {}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OperationIds {
    pub operation_ids: OperationHashSet,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PrivateKeys {
    pub keys: Vec<PrivateKey>,
}

/// This function sets up all the routes that can be used
/// and combines them into one filter
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
        .and(warp::path::param::<BlockId>()) // block id
        .and(warp::path::end())
        .and_then(move |hash| wrap_api_call(get_block(evt_tx.clone(), hash, storage.clone())));

    let evt_tx = event_tx.clone();
    let storage = opt_storage_command_sender.clone();
    let operations = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("get_operations"))
        .and(warp::path::end())
        .and(serde_qs::warp::query(serde_qs::Config::default()))
        .and_then(move |OperationIds { operation_ids }| {
            wrap_api_call(get_operations(
                evt_tx.clone(),
                operation_ids,
                storage.clone(),
            ))
        });

    let evt_tx = event_tx.clone();
    let consensus_cfg = consensus_config.clone();

    let storage = opt_storage_command_sender.clone();
    let blockinterval = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("blockinterval"))
        .and(warp::path::end())
        .and(warp::query::<TimeInterval>()) // start, end
        .and_then(move |TimeInterval { start, end }| {
            wrap_api_call(get_block_interval(
                evt_tx.clone(),
                consensus_cfg.clone(),
                start,
                end,
                storage.clone(),
            ))
        });

    let evt_tx = event_tx.clone();
    let current_parents = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("current_parents"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_current_parents(evt_tx.clone())));

    let evt_tx = event_tx.clone();
    let last_final = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_final"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_last_final(evt_tx.clone())));

    let evt_tx = event_tx.clone();
    let consensus_cfg = consensus_config.clone();
    let storage = opt_storage_command_sender.clone();
    let graph_interval = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("graph_interval"))
        .and(warp::path::end())
        .and(warp::query::<TimeInterval>()) // start, end
        .and_then(move |TimeInterval { start, end }| {
            wrap_api_call(get_graph_interval(
                evt_tx.clone(),
                consensus_cfg.clone(),
                start,
                end,
                storage.clone(),
            ))
        });

    let api_cfg = api_config.clone();
    let evt_tx = event_tx.clone();
    let consensus_cfg = consensus_config.clone();
    let storage = opt_storage_command_sender.clone();
    let graph_latest = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("graph_latest"))
        .and(warp::path::end())
        .and_then(move || {
            wrap_api_call(get_graph_latest(
                clock_compensation,
                api_cfg.graph_latest_timespan,
                evt_tx.clone(),
                consensus_cfg.clone(),
                storage.clone(),
            ))
        });

    let evt_tx = event_tx.clone();
    let storage = opt_storage_command_sender.clone();
    let block_ids_by_creator = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("block_ids_by_creator"))
        .and(warp::path::param::<Address>())
        .and(warp::path::end())
        .and_then(move |addr| {
            wrap_api_call(get_block_ids_by_creator(
                evt_tx.clone(),
                addr,
                storage.clone(),
            ))
        });

    let evt_tx = event_tx.clone();
    let cliques = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("cliques"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_cliques(evt_tx.clone())));

    let evt_tx = event_tx.clone();
    let peers = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("peers"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_peers(evt_tx.clone())));

    let network_cfg = network_config.clone();
    let our_ip = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("our_ip"))
        .and_then(move || wrap_api_call(get_our_ip(network_cfg.clone())));

    let evt_tx = event_tx.clone();
    let network_cfg = network_config.clone();
    let network_info = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("network_info"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_network_info(network_cfg.clone(), evt_tx.clone())));

    let node_config = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("node_config"))
        .and(warp::path::end())
        .and_then(|| wrap_api_call(get_node_config()));

    let version = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("version"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_version(node_version)));

    let pool_cfg = pool_config;
    let pool_config = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("pool_config"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_pool_config(pool_cfg.clone())));

    let consensus_cfg = consensus_config.clone();
    let get_consensus_cfg = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("consensus_config"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_consensus_config(consensus_cfg.clone())));

    let evt_tx = event_tx.clone();
    let network_cfg = network_config;
    let consensus_cfg = consensus_config.clone();
    let state = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("state"))
        .and(warp::path::end())
        .and_then(move || {
            wrap_api_call(get_state(
                evt_tx.clone(),
                consensus_cfg.clone(),
                network_cfg.clone(),
                clock_compensation,
            ))
        });

    let evt_tx = event_tx.clone();
    let api_cfg = api_config.clone();
    let last_stale = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_stale"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_last_stale(evt_tx.clone(), api_cfg.clone())));

    let evt_tx = event_tx.clone();
    let api_cfg = api_config.clone();
    let last_invalid = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("last_invalid"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_last_invalid(evt_tx.clone(), api_cfg.clone())));

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
            wrap_api_call(get_staker_info(
                evt_tx.clone(),
                api_cfg.clone(),
                consensus_cfg.clone(),
                creator,
                clock_compensation,
            ))
        });

    let evt_tx = event_tx.clone();
    let staker_stats = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("staker_stats"))
        .and(warp::path::end())
        .and(serde_qs::warp::query(serde_qs::Config::default()))
        .and_then(move |Addresses { addrs }| {
            wrap_api_call(get_stakers_stats(evt_tx.clone(), addrs))
        });

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
            wrap_api_call(get_next_draws(
                evt_tx.clone(),
                api_cfg.clone(),
                consensus_cfg.clone(),
                addrs,
                clock_compensation,
            ))
        });

    let evt_tx = event_tx.clone();
    let storage = opt_storage_command_sender;
    let operations_involving_address = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("operations_involving_address"))
        .and(warp::path::param::<Address>())
        .and(warp::path::end())
        .and_then(move |address| {
            wrap_api_call(get_operations_involving_address(
                evt_tx.clone(),
                address,
                storage.clone(),
            ))
        });

    let evt_tx = event_tx.clone();
    let addresses_info = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("addresses_info"))
        .and(warp::path::end())
        .and(serde_qs::warp::query(serde_qs::Config::default()))
        .and_then(move |Addresses { addrs }| {
            wrap_api_call(get_addresses_info(addrs, evt_tx.clone()))
        });

    let evt_tx = event_tx.clone();
    let stop_node = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("stop_node"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(stop_node(evt_tx.clone())));

    let evt_tx = event_tx.clone();
    let unban = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("unban"))
        .and(warp::path::param::<IpAddr>())
        .and(warp::path::end())
        .and_then(move |ip| wrap_api_call(unban(evt_tx.clone(), ip)));

    let evt_tx = event_tx.clone();
    let register_staking_private_keys = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("register_staking_keys"))
        .and(warp::path::end())
        .and(serde_qs::warp::query(serde_qs::Config::default()))
        .and_then(move |PrivateKeys { keys }| {
            wrap_api_call(register_staking_private_keys(evt_tx.clone(), keys))
        });

    let evt_tx = event_tx.clone();
    let remove_staking_addresses = warp::delete()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("remove_staking_addresses"))
        .and(warp::path::end())
        .and(serde_qs::warp::query(serde_qs::Config::default()))
        .and_then(move |Addresses { addrs }| {
            wrap_api_call(remove_staking_addresses(evt_tx.clone(), addrs))
        });

    let evt_tx = event_tx.clone();
    let send_operations = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("send_operations"))
        .and(warp::path::end())
        .and(warp::body::json())
        .and_then(move |operations| wrap_api_call(send_operations(operations, evt_tx.clone())));

    let evt_tx = event_tx.clone();
    let get_stats = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("get_stats"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_stats(evt_tx.clone())));

    let evt_tx = event_tx.clone();
    let get_active_stakers = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("active_stakers"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_active_stakers(evt_tx.clone())));

    let evt_tx = event_tx.clone();
    let staking_addresses = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("staking_addresses"))
        .and(warp::path::end())
        .and_then(move || wrap_api_call(get_staking_addresses(evt_tx.clone())));

    let evt_tx = event_tx;
    let node_sign_message = warp::post()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("node_sign_message"))
        .and(warp::path::end())
        .and(warp::body::bytes())
        .and_then(move |msg: warp::hyper::body::Bytes| {
            wrap_api_call(node_sign_msg(msg.to_vec(), evt_tx.clone()))
        });

    block
        .or(blockinterval)
        .or(current_parents)
        .or(last_final)
        .or(graph_interval)
        .or(graph_latest)
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
                "message": e.to_string()
            })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response(),
    })
}

async fn get_pool_config(config: PoolConfig) -> Result<PoolConfig, ApiError> {
    massa_trace!("api.filters.get_pool_config", {});
    Ok(config)
}

/// Returns our ip address
///
/// Note: as our ip address is in the config,
/// this function is more about getting every bit of
/// information we want exactly in the same way
async fn get_node_config() -> Result<SerializationContext, ApiError> {
    massa_trace!("api.filters.get_node_config", {});
    let context = models::with_serialization_context(|context| context.clone());
    Ok(context)
}

async fn get_version(version: Version) -> Result<Version, ApiError> {
    massa_trace!("api.filters.get_version", {});
    Ok(version)
}
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DataConsensusConfig {
    pub t0: UTime,
    pub thread_count: u8,
    pub genesis_timestamp: UTime,
    pub delta_f0: u64,
    pub max_block_size: u32,
    pub operation_validity_periods: u64,
    pub periods_per_cycle: u64,
    pub roll_price: Amount,
}
async fn get_consensus_config(
    consensus_cfg: ConsensusConfig,
) -> Result<DataConsensusConfig, ApiError> {
    massa_trace!("api.filters.get_consensus_config", {});
    Ok(DataConsensusConfig {
        t0: consensus_cfg.t0,
        thread_count: consensus_cfg.thread_count,
        genesis_timestamp: consensus_cfg.genesis_timestamp,
        delta_f0: consensus_cfg.delta_f0,
        max_block_size: consensus_cfg.max_block_size,
        operation_validity_periods: consensus_cfg.operation_validity_periods,
        periods_per_cycle: consensus_cfg.periods_per_cycle,
        roll_price: consensus_cfg.roll_price,
    })
}

/// This function sends AskStop outside the Api and
/// return the result as a warp reply.
///
/// # Argument
/// * `event_tx`: Sender used to send the event out
async fn stop_node(evt_tx: mpsc::Sender<ApiEvent>) -> Result<(), ApiError> {
    massa_trace!("api.filters.stop_node", {});
    Ok(evt_tx
        .send(ApiEvent::AskStop)
        .await
        .map_err(|e| ApiError::SendChannelError(format!("{}", e)))?)
}

async fn unban(evt_tx: mpsc::Sender<ApiEvent>, ip: IpAddr) -> Result<(), ApiError> {
    massa_trace!("api.filters.unban", {});
    Ok(evt_tx
        .send(ApiEvent::Unban(ip))
        .await
        .map_err(|e| ApiError::SendChannelError(format!("{}", e)))?)
}

async fn register_staking_private_keys(
    evt_tx: mpsc::Sender<ApiEvent>,
    keys: Vec<PrivateKey>,
) -> Result<(), ApiError> {
    massa_trace!("api.filters.register_staking_private_keys", {});

    Ok(evt_tx
        .send(ApiEvent::RegisterStakingPrivateKeys(keys))
        .await
        .map_err(|e| ApiError::SendChannelError(format!("{}", e)))?)
}

async fn remove_staking_addresses(
    evt_tx: mpsc::Sender<ApiEvent>,
    addrs: AddressHashSet,
) -> Result<(), ApiError> {
    massa_trace!("api.filters.remove_staking_addresses", {});
    Ok(evt_tx
        .send(ApiEvent::RemoveStakingAddresses(addrs))
        .await
        .map_err(|e| ApiError::SendChannelError(format!("{}", e)))?)
}

/// This function sends the new transaction outside the Api and
/// return the result as a warp reply.
///
/// The transaction is verified before been propagated.
/// # Argument
/// * `event_tx`: Sender used to send the event out
async fn send_operations(
    operations: Vec<Operation>,
    evt_tx: mpsc::Sender<ApiEvent>,
) -> Result<Vec<OperationId>, ApiError> {
    massa_trace!("api.filters.send_operations ", { "operations": operations });
    let to_send = operations
        .into_iter()
        .map(|op| Ok((op.verify_integrity()?, op)))
        .collect::<Result<OperationHashMap<Operation>, ApiError>>()?;

    let opid_list = to_send
        .iter()
        .map(|(opid, _)| *opid)
        .collect::<Vec<OperationId>>();

    evt_tx
        .send(ApiEvent::AddOperations(to_send))
        .await
        .map_err(|e| ApiError::SendChannelError(format!("{}", e)))?;
    Ok(opid_list)
}

/// Returns block with given hash as a reply
async fn get_block(
    event_tx: mpsc::Sender<ApiEvent>,
    block_id: BlockId,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<Option<ExportBlockStatus>, ApiError> {
    massa_trace!("api.filters.get_block", { "block_id": block_id });
    if let Some(block) = retrieve_block(block_id, &event_tx).await? {
        Ok(Some(block))
    } else {
        if let Some(cmd_tx) = opt_storage_command_sender {
            match cmd_tx.get_block(block_id).await {
                // TODO: this is blocking the removal of Stored variant.
                Ok(Some(block)) => Ok(Some(ExportBlockStatus::Stored(block))),
                Ok(None) => Err(ApiError::NotFound),
                Err(e) => Err(e.into()),
            }
        } else {
            Err(ApiError::NotFound)
        }
    }
}

async fn get_block_ids_by_creator(
    event_tx: mpsc::Sender<ApiEvent>,
    address: Address,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<BlockHashMap<Status>, ApiError> {
    massa_trace!("api.filters.get_block_ids_by_creator", {
        "address": address
    });
    let mut res = retrieve_block_ids_by_creator_from_consensus(address, &event_tx).await?;

    if let Some(cmd_tx) = opt_storage_command_sender {
        res.extend(retrieve_block_ids_by_creator_from_storage(address, &cmd_tx).await?)
    }

    Ok(res)
}

async fn get_operations(
    event_tx: mpsc::Sender<ApiEvent>,
    operation_ids: OperationHashSet,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<Vec<(OperationId, OperationSearchResult)>, ApiError> {
    massa_trace!("api.filters.get_operations", {
        "operation_ids": operation_ids
    });
    let mut res: Vec<(OperationId, OperationSearchResult)> =
        retrieve_operations(operation_ids.clone(), &event_tx)
            .await
            .map(|map| map.into_iter().collect())?;

    if let Some(access) = opt_storage_command_sender {
        res.extend(access.get_operations(operation_ids).await?);
    }
    Ok(res)
}

async fn do_node_sign_msg(
    message: Vec<u8>,
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<PubkeySig, ApiError> {
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::NodeSignMessage {
            message,
            response_tx,
        })
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!(
                "Could not send api event node sign message: {0}",
                e
            ))
        })?;
    response_rx.await.map_err(|e| {
        ApiError::ReceiveChannelError(format!("Could not retrieve node message signature: {0}", e))
    })
}

pub async fn node_sign_msg(
    msg: Vec<u8>,
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<PubkeySig, ApiError> {
    massa_trace!("api.filters.node_sign_msg", { "msg": msg });
    Ok(do_node_sign_msg(msg, &event_tx).await?)
}

/// Returns our ip address
///
/// Note: as our ip address is in the config,
/// this function is more about getting every bit of
/// information we want exactly in the same way
async fn get_our_ip(network_cfg: NetworkConfig) -> Result<Option<IpAddr>, ApiError> {
    massa_trace!("api.filters.get_our_ip", {});
    Ok(network_cfg.routable_ip)
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
            ApiError::SendChannelError(format!("could not send api event get block graph: {0}", e))
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
            ApiError::SendChannelError(format!("Could not send api event get block: {0}", e))
        })?;
    response_rx
        .await
        .map_err(|e| ApiError::ReceiveChannelError(format!("Could not retrieve block: {0}", e)))
}

async fn retrieve_block_ids_by_creator_from_consensus(
    address: Address,
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<BlockHashMap<Status>, ApiError> {
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
        ApiError::ReceiveChannelError(format!("Could not retrieve GetBlockIdsByCreator: {0}", e))
    })
}

async fn retrieve_block_ids_by_creator_from_storage(
    address: Address,
    storage_access: &StorageAccess,
) -> Result<BlockHashMap<Status>, ApiError> {
    Ok(storage_access
        .get_block_ids_by_creator(&address)
        .await?
        .into_iter()
        .map(|id| (id, Status::Final))
        .collect())
}

async fn retrieve_operations(
    operation_ids: OperationHashSet,
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<OperationHashMap<OperationSearchResult>, ApiError> {
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
            ApiError::SendChannelError(format!("Could not send api event get operation: {0}", e))
        })?;
    response_rx
        .await
        .map_err(|e| ApiError::ReceiveChannelError(format!("Could not retrieve operation: {0}", e)))
}

async fn retrieve_peers(event_tx: &mpsc::Sender<ApiEvent>) -> Result<Peers, ApiError> {
    massa_trace!("api.filters.retrieve_peers", {});
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetPeers(response_tx))
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!("Could not send api event get peers: {0}", e))
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
async fn get_current_parents(
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<Vec<(BlockId, Slot)>, ApiError> {
    massa_trace!("api.filters.get_current_parents", {});
    let graph = retrieve_graph_export(&event_tx).await?;

    let parents = graph.best_parents;
    let mut best = Vec::new();
    for (hash, _) in parents {
        match graph.active_blocks.get_key_value(&hash) {
            Some((_, block)) => best.push((hash, block.block.content.slot)),
            None => {
                return Err(ApiError::InconsistencyError(
                    "inconsistency error between best_parents and active_blocks".to_string(),
                ))
            }
        }
    }

    Ok(best)
}

/// Returns last final blocks wrapped in a reply.
async fn get_last_final(
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<Vec<(BlockId, Slot)>, ApiError> {
    massa_trace!("api.filters.get_last_final", {});
    let graph = retrieve_graph_export(&event_tx).await?;

    let finals = graph
        .latest_final_blocks_periods
        .iter()
        .enumerate()
        .map(|(i, (hash, period))| (*hash, Slot::new(*period, i as u8)))
        .collect();
    Ok(finals)
}

async fn get_block_from_graph(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: &ConsensusConfig,
    start_opt: Option<UTime>,
    end_opt: Option<UTime>,
) -> Result<Vec<(BlockId, Slot)>, ApiError> {
    massa_trace!("api.filters.get_block_from_graph", {});
    let graph = retrieve_graph_export(&event_tx).await?;

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
            .map_err(|e| ApiError::from(e))
            .map(|time| {
                if start <= time && time < end {
                    Some((hash, exported_block.block.content.slot))
                } else {
                    None
                }
            })
            .transpose()
        })
        .collect::<Result<Vec<(BlockId, Slot)>, ApiError>>()
}

async fn get_block_interval(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: ConsensusConfig,
    start_opt: Option<UTime>,
    end_opt: Option<UTime>,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<Vec<(BlockId, Slot)>, ApiError> {
    massa_trace!("api.filters.get_block_interval", {});
    if start_opt
        .and_then(|s| end_opt.and_then(|e| if s >= e { Some(()) } else { None }))
        .is_some()
    {
        return Ok(vec![]);
    }

    // filter block from graph_export
    let mut res = get_block_from_graph(event_tx, &consensus_cfg, start_opt, end_opt).await?;

    if let Some(ref storage) = opt_storage_command_sender {
        let (start_slot, end_slot) = time_range_to_slot_range(
            consensus_cfg.thread_count,
            consensus_cfg.t0,
            consensus_cfg.genesis_timestamp,
            start_opt,
            end_opt,
        )?;

        storage
            .get_slot_range(start_slot, end_slot)
            .await
            .map(|blocks| {
                res.append(
                    &mut blocks
                        .into_iter()
                        .map(|(hash, block)| (hash, block.header.content.slot))
                        .collect(),
                )
            })?;
    }

    Ok(res)
}

/// Returns all block info needed to reconstruct the most recent part of the block graph.
/// The result is a vec of (hash, period, thread, status, parents hash) wrapped in a reply.
///
/// Note:
/// * both start time is included and end time is excluded
/// * status is in `["active", "final", "stale"]`
async fn get_graph_latest(
    clock_compensation: i64,
    graph_latest_timespan: UTime,
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: ConsensusConfig,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<Vec<(BlockId, Slot, Status, Vec<BlockId>)>, ApiError> {
    let time_end = UTime::now(clock_compensation)?;
    let time_start = time_end.saturating_sub(graph_latest_timespan);
    get_graph_interval(
        event_tx,
        consensus_cfg,
        Some(time_start),
        Some(time_end),
        opt_storage_command_sender,
    )
    .await
}

/// Returns all block info needed to reconstruct the graph found in the time interval.
/// The result is a vec of (hash, period, thread, status, parents hash) wrapped in a reply.
///
/// Note:
/// * both start time is included and end time is excluded
/// * status is in `["active", "final", "stale"]`
async fn get_graph_interval(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: ConsensusConfig,
    start_opt: Option<UTime>,
    end_opt: Option<UTime>,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<Vec<(BlockId, Slot, Status, Vec<BlockId>)>, ApiError> {
    massa_trace!("api.filters.get_graph_interval_process", {});
    // filter block from graph_export
    let graph = retrieve_graph_export(&event_tx).await?;
    let start = start_opt.unwrap_or_else(|| UTime::from(0));
    let end = end_opt.unwrap_or_else(|| UTime::from(u64::MAX));
    let mut res = Vec::new();
    for (hash, exported_block) in graph.active_blocks.into_iter() {
        let header = exported_block.block;
        let status = exported_block.status;
        let time = get_block_slot_timestamp(
            consensus_cfg.thread_count,
            consensus_cfg.t0,
            consensus_cfg.genesis_timestamp,
            header.content.slot,
        )?;

        if start <= time && time < end {
            res.push((hash, header.content.slot, status, header.content.parents))
        }
    }

    if let Some(storage) = opt_storage_command_sender {
        let (start_slot, end_slot) = time_range_to_slot_range(
            consensus_cfg.thread_count,
            consensus_cfg.t0,
            consensus_cfg.genesis_timestamp,
            start_opt,
            end_opt,
        )?;

        let blocks = storage.get_slot_range(start_slot, end_slot).await?;
        for (hash, block) in blocks {
            res.push((
                hash,
                block.header.content.slot,
                Status::Final,
                block.header.content.parents.clone(),
            ));
        }
    }
    Ok(res)
}

/// Returns number of cliques and current cliques as `Vec<HashSet<(hash, (period, thread))>>`
/// The result is a tuple `(number_of_cliques, current_cliques)` wrapped in a reply.
async fn get_cliques(
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<(usize, Vec<HashSet<(BlockId, Slot)>>), ApiError> {
    massa_trace!("api.filters.get_cliques", {});
    let graph = retrieve_graph_export(&event_tx).await?;

    let mut hashes = BlockHashSet::default();
    for clique in graph.max_cliques.iter() {
        hashes.extend(&clique.block_ids)
    }

    let mut hashes_map = BlockHashMap::default();
    for hash in hashes.iter() {
        if let Some((_, block)) = graph.active_blocks.get_key_value(hash) {
            hashes_map.insert(*hash, block.block.content.slot);
        } else {
            return Err(ApiError::InconsistencyError(
                "inconstancy error between cliques and active_blocks".to_string(),
            ));
        }
    }

    let mut res = Vec::new();
    for clique in graph.max_cliques.iter() {
        let mut set = HashSet::new();
        for hash in clique.block_ids.iter() {
            match hashes_map.get_key_value(hash) {
                Some((k, v)) => {
                    set.insert((*k, *v));
                }
                None => {
                    return Err(ApiError::InconsistencyError(
                        "inconstancy error between cliques and active_blocks".to_string(),
                    ));
                }
            }
        }
        res.push(set)
    }

    Ok((graph.max_cliques.len(), res))
}

#[derive(Clone, Serialize)]
struct NetworkInfo {
    our_ip: Option<IpAddr>,
    peers: HashMap<IpAddr, Peer>,
    node_id: NodeId,
}

/// Returns network information:
/// * own IP address
/// * connected peers :
///      - ip address
///      - peer info (see `struct PeerInfo` in `communication::network::PeerInfoDatabase`)
async fn get_network_info(
    network_cfg: NetworkConfig,
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<NetworkInfo, ApiError> {
    massa_trace!("api.filters.get_network_info", {});
    let peers = retrieve_peers(&event_tx).await?;
    let our_ip = network_cfg.routable_ip;
    Ok(NetworkInfo {
        our_ip,
        peers: peers.peers,
        node_id: peers.our_node_id,
    })
}

/// Returns state info for a set of addresses
async fn get_addresses_info(
    addresses: AddressHashSet,
    event_tx: mpsc::Sender<ApiEvent>,
) -> Result<AddressHashMap<AddressState>, ApiError> {
    massa_trace!("api.filters.get_addresses_info", { "addresses": addresses });
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetAddressesInfo {
            addresses,
            response_tx,
        })
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!(
                "Could not send api event get address info : {0}",
                e
            ))
        })?;

    let addrs_info = response_rx.await.map_err(|e| {
        ApiError::ReceiveChannelError(format!("Could not retrieve address info : {0}", e))
    })??;

    Ok(addrs_info)
}

/// Returns connected peers :
/// - ip address
/// - peer info (see `struct PeerInfo` in `communication::network::PeerInfoDatabase`)
async fn get_peers(event_tx: mpsc::Sender<ApiEvent>) -> Result<Peers, ApiError> {
    massa_trace!("api.filters.get_peers", {});
    let peers = retrieve_peers(&event_tx).await?;
    Ok(peers)
}

async fn get_operations_involving_address(
    event_tx: mpsc::Sender<ApiEvent>,
    address: Address,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<OperationHashMap<OperationSearchResult>, ApiError> {
    massa_trace!("api.filters.get_operations_involving_address", {
        "address": address
    });
    let (response_tx, response_rx) = oneshot::channel();
    event_tx
        .send(ApiEvent::GetRecentOperations {
            address,
            response_tx,
        })
        .await
        .map_err(|e| {
            ApiError::SendChannelError(format!(
                "could not send api event get operation involving address : {0}",
                e
            ))
        })?;

    let mut res = response_rx.await.map_err(|e| {
        ApiError::ReceiveChannelError(format!(
            "could not retrieve operation involving address: {0}",
            e
        ))
    })?;

    if let Some(access) = &opt_storage_command_sender {
        access
            .get_operations_involving_address(&address)
            .await
            .expect("could not retrieve recent operations from storage")
            .into_iter()
            .for_each(|(op_id, search_new)| {
                res.entry(op_id)
                    .and_modify(|search_old| search_old.extend(&search_new))
                    .or_insert(search_new);
            })
    }

    Ok(res)
}

#[derive(Clone, Serialize)]
pub struct State {
    time: UTime,
    latest_slot: Option<Slot>,
    current_cycle: u64,
    our_ip: Option<IpAddr>,
    last_final: Vec<(BlockId, Slot, UTime)>,
    nb_cliques: usize,
    nb_peers: usize,
}

/// Returns a summary of the current state:
/// * time in `UTime`
/// * latest slot (optional)
/// * last final block
/// * number of cliques
/// * number of connected peers
async fn get_state(
    event_tx: mpsc::Sender<ApiEvent>,
    consensus_cfg: ConsensusConfig,
    network_cfg: NetworkConfig,
    clock_compensation: i64,
) -> Result<State, ApiError> {
    massa_trace!("api.filters.get_state", {});
    let cur_time = UTime::now(clock_compensation)?;

    let latest_slot_opt = get_latest_block_slot_at_timestamp(
        consensus_cfg.thread_count,
        consensus_cfg.t0,
        consensus_cfg.genesis_timestamp,
        cur_time,
    )?;

    let peers = retrieve_peers(&event_tx).await?;

    let connected_peers: HashSet<IpAddr> = peers
        .peers
        .iter()
        .filter(|(_ip, Peer { peer_info, .. })| {
            peer_info.active_out_connections > 0 || peer_info.active_in_connections > 0
        })
        .map(|(ip, _peer_info)| *ip)
        .collect();

    let graph = retrieve_graph_export(&event_tx).await?;

    let finals = graph
        .latest_final_blocks_periods
        .iter()
        .enumerate()
        .map(|(thread, (hash, period))| {
            Ok((
                *hash,
                Slot::new(*period, thread as u8),
                get_block_slot_timestamp(
                    consensus_cfg.thread_count,
                    consensus_cfg.t0,
                    consensus_cfg.genesis_timestamp,
                    Slot::new(*period, thread as u8),
                )?,
            ))
        })
        .collect::<Result<Vec<(BlockId, Slot, UTime)>, consensus::ConsensusError>>()?;

    Ok(State {
        time: cur_time,
        latest_slot: latest_slot_opt,
        current_cycle: latest_slot_opt
            .unwrap_or_else(|| Slot::new(0, 0))
            .get_cycle(consensus_cfg.periods_per_cycle),
        our_ip: network_cfg.routable_ip,
        last_final: finals,
        nb_cliques: graph.max_cliques.len(),
        nb_peers: connected_peers.len(),
    })
}

/// Returns a number of last stale blocks as a `Vec<(Hash, Slot)>` wrapped in a reply.
async fn get_last_stale(
    event_tx: mpsc::Sender<ApiEvent>,
    api_config: ApiConfig,
) -> Result<Vec<(BlockId, Slot)>, ApiError> {
    massa_trace!("api.filters.get_last_stale", {});
    let graph = retrieve_graph_export(&event_tx).await?;

    let discarded = graph.discarded_blocks;
    let mut discarded = discarded
        .iter()
        .filter(|(_hash, (reason, _header))| *reason == DiscardReason::Stale)
        .map(|(hash, (_reason, header))| (*hash, header.content.slot))
        .collect::<Vec<(BlockId, Slot)>>();
    if !discarded.is_empty() {
        let min = min(discarded.len(), api_config.max_return_invalid_blocks);
        discarded = discarded.drain(0..min).collect();
    }

    Ok(discarded)
}

/// Returns a number of last invalid blocks as a `Vec<(Hash, Slot)>` wrapped in a reply.
async fn get_last_invalid(
    event_tx: mpsc::Sender<ApiEvent>,
    api_cfg: ApiConfig,
) -> Result<Vec<(BlockId, Slot)>, ApiError> {
    massa_trace!("api.filters.get_last_invalid", {});

    let graph = retrieve_graph_export(&event_tx).await?;
    let discarded = graph.discarded_blocks;
    let mut discarded = discarded
        .iter()
        .filter(|(_hash, (reason, _header))| matches!(reason, DiscardReason::Invalid(_)))
        .map(|(hash, (_reason, header))| (*hash, header.content.slot))
        .collect::<Vec<(BlockId, Slot)>>();
    if !discarded.is_empty() {
        let min = min(discarded.len(), api_cfg.max_return_invalid_blocks);
        discarded = discarded.drain(0..min).collect();
    }

    Ok(discarded)
}

#[derive(Clone, Serialize)]
pub struct StakerInfo {
    staker_active_blocks: Vec<(BlockId, BlockHeader)>,
    staker_discarded_blocks: Vec<(BlockId, DiscardReason, BlockHeader)>,
    staker_next_draws: Vec<Slot>,
}

/// Returns
/// * a number of discarded blocks by the staker as a `Vec<(&Hash, DiscardReason, BlockHeader)>`
/// * a number of active blocks by the staker as a `Vec<(&Hash, BlockHeader)>`
/// * next slots that are for the staker as a `Vec<Slot>`
async fn get_staker_info(
    event_tx: mpsc::Sender<ApiEvent>,
    api_cfg: ApiConfig,
    consensus_cfg: ConsensusConfig,
    creator: Address,
    clock_compensation: i64,
) -> Result<StakerInfo, ApiError> {
    massa_trace!("api.filters.get_staker_info", {});
    let graph = retrieve_graph_export(&event_tx).await?;

    let blocks = graph
        .active_blocks
        .iter()
        .filter(|(_hash, block)| {
            Address::from_public_key(&block.block.content.creator).unwrap() == creator
        })
        .map(|(hash, block)| (*hash, block.block.clone()))
        .collect::<Vec<(BlockId, BlockHeader)>>();

    let discarded = graph
        .discarded_blocks
        .iter()
        .filter(|(_hash, (_reason, header))| {
            Address::from_public_key(&header.content.creator).unwrap() == creator
        })
        .map(|(hash, (reason, header))| (*hash, reason.clone(), header.clone()))
        .collect::<Vec<(BlockId, DiscardReason, BlockHeader)>>();
    let cur_time = UTime::now(clock_compensation)?;

    let start_slot = get_latest_block_slot_at_timestamp(
        consensus_cfg.thread_count,
        consensus_cfg.t0,
        consensus_cfg.genesis_timestamp,
        cur_time,
    )?
    .unwrap_or_else(|| Slot::new(0, 0));
    let end_slot = Slot::new(
        start_slot
            .period
            .saturating_add(api_cfg.selection_return_periods),
        start_slot.thread,
    );

    let next_slots_by_creator: Vec<Slot> = retrieve_selection_draw(start_slot, end_slot, &event_tx)
        .await?
        .into_iter()
        .filter_map(|(slt, (sel, _))| {
            // TODO: retrieve next endorsement by staker?
            if sel == creator {
                return Some(slt);
            }
            None
        })
        .collect();

    Ok(StakerInfo {
        staker_active_blocks: blocks,
        staker_discarded_blocks: discarded,
        staker_next_draws: next_slots_by_creator,
    })
}

/// Returns staker production stats
async fn get_stakers_stats(
    event_tx: mpsc::Sender<ApiEvent>,
    addrs: AddressHashSet,
) -> Result<Vec<StakersCycleProductionStats>, ApiError> {
    massa_trace!("api.filters.get_stakers_stats", { "addrs": addrs });
    retrieve_stakers_production_stats(addrs, &event_tx).await
}

async fn get_next_draws(
    event_tx: mpsc::Sender<ApiEvent>,
    api_cfg: ApiConfig,
    consensus_cfg: ConsensusConfig,
    addresses: AddressHashSet,
    clock_compensation: i64,
) -> Result<Vec<(Address, Slot)>, ApiError> {
    let cur_time = UTime::now(clock_compensation)?;

    let start_slot = get_latest_block_slot_at_timestamp(
        consensus_cfg.thread_count,
        consensus_cfg.t0,
        consensus_cfg.genesis_timestamp,
        cur_time,
    )?
    .unwrap_or_else(|| Slot::new(0, 0));
    let end_slot = Slot::new(
        start_slot
            .period
            .saturating_add(api_cfg.selection_return_periods),
        start_slot.thread,
    );

    let next_slots: Vec<(Address, Slot)> = retrieve_selection_draw(start_slot, end_slot, &event_tx)
        .await?
        .into_iter()
        .filter_map(|(slt, (sel, _))| {
            if addresses.contains(&sel) {
                // TODO: retrieve endorsements by addresses?
                return Some((sel, slt));
            }
            None
        })
        .collect();

    Ok(next_slots)
}

async fn retrieve_stakers_production_stats(
    addrs: AddressHashSet,
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
            "Could not retrieve stakers production stats: {0}",
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

async fn get_stats(event_tx: mpsc::Sender<ApiEvent>) -> Result<ConsensusStats, ApiError> {
    massa_trace!("api.filters.get_stats", {});
    retrieve_stats(&event_tx).await
}

async fn retrieve_staking_addresses(
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<AddressHashSet, ApiError> {
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
) -> Result<AddressHashSet, ApiError> {
    massa_trace!("api.filters.get_staking_addresses", {});
    retrieve_staking_addresses(&event_tx).await
}

async fn retrieve_active_stakers(
    event_tx: &mpsc::Sender<ApiEvent>,
) -> Result<Option<AddressHashMap<u64>>, ApiError> {
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
) -> Result<Option<AddressHashMap<u64>>, ApiError> {
    massa_trace!("api.filters.get_active_stakers", {});
    retrieve_active_stakers(&event_tx).await
}
