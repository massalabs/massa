// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::error::ApiError;
use crate::error::ApiError::WrongAPI;
use crate::{Endpoints, Public, RpcServer, StopHandle, API};
use consensus::{ConsensusCommandSender, ConsensusConfig, ExportBlockStatus, Status};
use crypto::signature::PrivateKey;
use jsonrpc_core::BoxFuture;
use models::address::{AddressHashMap, AddressHashSet};
use models::api::{
    APIConfig, AddressInfo, BalanceInfo, BlockInfo, BlockInfoContent, BlockSummary,
    EndorsementInfo, NodeStatus, OperationInfo, RollsInfo, TimeInterval,
};
use models::clique::Clique;
use models::crypto::PubkeySig;
use models::node::NodeId;
use models::timeslots::{
    get_block_slot_timestamp, get_latest_block_slot_at_timestamp, time_range_to_slot_range,
};
use models::{
    Address, BlockHashSet, BlockId, EndorsementId, Operation, OperationHashMap, OperationHashSet,
    OperationId, Slot, Version,
};
use network::{NetworkCommandSender, NetworkConfig};
use pool::PoolCommandSender;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use storage::StorageAccess;
use time::UTime;

impl API<Public> {
    pub fn new(
        consensus_command_sender: ConsensusCommandSender,
        api_config: APIConfig,
        consensus_config: ConsensusConfig,
        pool_command_sender: PoolCommandSender,
        storage_command_sender: Option<StorageAccess>,
        network_config: NetworkConfig,
        version: Version,
        network_command_sender: NetworkCommandSender,
        compensation_millis: i64,
        node_id: NodeId,
    ) -> Self {
        API(Public {
            consensus_command_sender,
            consensus_config,
            api_config,
            pool_command_sender,
            storage_command_sender,
            network_config,
            version,
            network_command_sender,
            compensation_millis,
            node_id,
        })
    }
}

impl RpcServer for API<Public> {
    fn serve(self, url: &SocketAddr) -> StopHandle {
        crate::serve(self, url)
    }
}

#[doc(hidden)]
impl Endpoints for API<Public> {
    fn stop_node(&self) -> BoxFuture<Result<(), ApiError>> {
        let closure = async move || Err(WrongAPI);
        Box::pin(closure())
    }

    fn node_sign_message(&self, _: Vec<u8>) -> BoxFuture<Result<PubkeySig, ApiError>> {
        let closure = async move || Err(WrongAPI);
        Box::pin(closure())
    }

    fn add_staking_private_keys(&self, _: Vec<PrivateKey>) -> BoxFuture<Result<(), ApiError>> {
        let closure = async move || Err(WrongAPI);
        Box::pin(closure())
    }

    fn remove_staking_addresses(&self, _: Vec<Address>) -> BoxFuture<Result<(), ApiError>> {
        let closure = async move || Err(WrongAPI);
        Box::pin(closure())
    }

    fn get_staking_addresses(&self) -> BoxFuture<Result<AddressHashSet, ApiError>> {
        let closure = async move || Err(WrongAPI);
        Box::pin(closure())
    }

    fn ban(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        let closure = async move || Err(WrongAPI);
        Box::pin(closure())
    }

    fn unban(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        let closure = async move || Err(WrongAPI);
        Box::pin(closure())
    }

    fn get_status(&self) -> BoxFuture<Result<NodeStatus, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let network_command_sender = self.0.network_command_sender.clone();
        let network_config = self.0.network_config.clone();
        let version = self.0.version;
        let consensus_config = self.0.consensus_config.clone();
        let compensation_millis = self.0.compensation_millis;
        let mut pool_command_sender = self.0.pool_command_sender.clone();
        let node_id = self.0.node_id;
        let algo_config = consensus_config.to_algo_config();
        let closure = async move || {
            let now = UTime::now(compensation_millis)?;
            let last_slot = get_latest_block_slot_at_timestamp(
                consensus_config.thread_count,
                consensus_config.t0,
                consensus_config.genesis_timestamp,
                now,
            )?;

            let (consensus_stats, network_stats, pool_stats, peers) = tokio::join!(
                consensus_command_sender.get_stats(),
                network_command_sender.get_network_stats(),
                pool_command_sender.get_pool_stats(),
                network_command_sender.get_peers()
            );
            Ok(NodeStatus {
                node_id,
                node_ip: network_config.routable_ip,
                version,
                genesis_timestamp: consensus_config.genesis_timestamp,
                t0: consensus_config.t0,
                delta_f0: consensus_config.delta_f0,
                roll_price: consensus_config.roll_price,
                thread_count: consensus_config.thread_count,
                current_time: now,
                connected_nodes: peers?
                    .peers
                    .iter()
                    .map(|(ip, peer)| peer.active_nodes.iter().map(move |(id, _)| (*id, *ip)))
                    .flatten()
                    .collect(),
                last_slot,
                next_slot: last_slot
                    .unwrap_or_else(|| Slot::new(0, 0))
                    .get_next_slot(consensus_config.thread_count)?,
                consensus_stats: consensus_stats?,
                network_stats: network_stats?,
                pool_stats: pool_stats?,

                algo_config,
                current_cycle: last_slot
                    .unwrap_or(Slot::new(0, 0))
                    .get_cycle(consensus_config.periods_per_cycle),
            })
        };
        Box::pin(closure())
    }

    fn get_cliques(&self) -> BoxFuture<Result<Vec<Clique>, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let closure = async move || {
            Ok(consensus_command_sender
                .get_block_graph_status()
                .await?
                .max_cliques)
        };
        Box::pin(closure())
    }

    fn get_stakers(&self) -> BoxFuture<Result<AddressHashMap<u64>, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let closure = async move || Ok(consensus_command_sender.get_active_stakers().await?);
        Box::pin(closure())
    }

    fn get_operations(
        &self,
        ops: Vec<OperationId>,
    ) -> BoxFuture<Result<Vec<OperationInfo>, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let mut pool_command_sender = self.0.pool_command_sender.clone();
        let storage_command_sender = self.0.storage_command_sender.clone();
        let closure = async move || {
            let mut res: OperationHashMap<OperationInfo> = pool_command_sender
                .get_operations(ops.iter().cloned().collect())
                .await?
                .into_iter()
                .map(|(id, operation)| {
                    (
                        id,
                        OperationInfo {
                            operation,
                            in_pool: true,
                            in_blocks: Vec::new(),
                            id,
                            is_final: false,
                        },
                    )
                })
                .collect();
            consensus_command_sender
                .get_operations(ops.iter().cloned().collect())
                .await?
                .into_iter()
                .for_each(|(op_id, search_new)| {
                    let search_new = OperationInfo {
                        id: op_id,
                        in_pool: search_new.in_pool,
                        in_blocks: search_new.in_blocks.keys().copied().collect(),
                        is_final: search_new
                            .in_blocks
                            .iter()
                            .any(|(_, (_, is_final))| *is_final),
                        operation: search_new.op,
                    };
                    res.entry(op_id)
                        .and_modify(|search_old| search_old.extend(&search_new))
                        .or_insert(search_new);
                });
            // for those that have not been found in consensus, extend with storage
            let to_gather: OperationHashSet = ops
                .iter()
                .filter(|op_id| {
                    if let Some(cur_search) = res.get(op_id) {
                        if !cur_search.in_blocks.is_empty() {
                            return false;
                        }
                    }
                    true
                })
                .copied()
                .collect();
            if let Some(storage_command_sender) = storage_command_sender {
                storage_command_sender
                    .get_operations(to_gather)
                    .await?
                    .into_iter()
                    .for_each(|(op_id, search_new)| {
                        let search_new = OperationInfo {
                            id: op_id,
                            in_pool: search_new.in_pool,
                            in_blocks: search_new.in_blocks.keys().copied().collect(),
                            is_final: search_new
                                .in_blocks
                                .iter()
                                .any(|(_, (_, is_final))| *is_final),
                            operation: search_new.op,
                        };
                        res.entry(op_id)
                            .and_modify(|search_old| search_old.extend(&search_new))
                            .or_insert(search_new);
                    });
            }
            Ok(res.into_values().collect())
        };
        Box::pin(closure())
    }

    fn get_endorsements(
        &self,
        _: Vec<EndorsementId>,
    ) -> BoxFuture<Result<Vec<EndorsementInfo>, ApiError>> {
        todo!() // TODO: wait for !238
    }

    fn get_blocks(&self, ids: Vec<BlockId>) -> BoxFuture<Result<Vec<BlockInfo>, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let opt_storage_command_sender = self.0.storage_command_sender.clone();
        let closure = async move || {
            let mut res = Vec::new();
            let graph = consensus_command_sender.get_block_graph_status().await?;
            let block_clique = graph
                .max_cliques
                .iter()
                .find(|clique| clique.is_blockclique)
                .ok_or_else(|| ApiError::InconsistencyError("Missing block clique".to_string()))?;
            for id in ids.into_iter() {
                if let Some((block, is_final)) =
                    match consensus_command_sender.get_block_status(id).await? {
                        Some(ExportBlockStatus::Active(block)) => Some((block, false)),
                        Some(ExportBlockStatus::Incoming) => None,
                        Some(ExportBlockStatus::WaitingForSlot) => None,
                        Some(ExportBlockStatus::WaitingForDependencies) => None,
                        Some(ExportBlockStatus::Discarded(_)) => None, // TODO: get block if stale
                        Some(ExportBlockStatus::Final(block)) => Some((block, true)),
                        None => None,
                    }
                {
                    res.push(BlockInfo {
                        id,
                        content: Some(BlockInfoContent {
                            is_final,
                            is_stale: false,
                            is_in_blockclique: block_clique.block_ids.contains(&id),
                            block,
                        }),
                    })
                } else if let Some(opt_storage_command_sender) = opt_storage_command_sender.clone()
                {
                    match opt_storage_command_sender.get_block(id).await {
                        Ok(Some(block)) => res.push(BlockInfo {
                            id,
                            content: Some(BlockInfoContent {
                                is_final: true,
                                is_stale: false,
                                is_in_blockclique: block_clique.block_ids.contains(&id),
                                block,
                            }),
                        }),
                        Ok(None) => res.push(BlockInfo { id, content: None }),
                        Err(e) => return Err(e.into()),
                    }
                }
            }
            Ok(res)
        };
        Box::pin(closure())
    }

    fn get_graph_interval(
        &self,
        time: TimeInterval,
    ) -> BoxFuture<Result<Vec<BlockSummary>, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let opt_storage_command_sender = self.0.storage_command_sender.clone();
        let consensus_config = self.0.consensus_config.clone();
        let closure = async move || {
            let graph = consensus_command_sender.get_block_graph_status().await?;
            // filter block from graph_export
            let start = time.start.unwrap_or_else(|| UTime::from(0));
            let end = time.end.unwrap_or_else(|| UTime::from(u64::MAX));
            let mut res = Vec::new();
            let block_clique = graph
                .max_cliques
                .iter()
                .find(|clique| clique.is_blockclique)
                .ok_or_else(|| ApiError::InconsistencyError("Missing block clique".to_string()))?;
            for (id, exported_block) in graph.active_blocks.into_iter() {
                let header = exported_block.block;
                let status = exported_block.status;
                let time = get_block_slot_timestamp(
                    consensus_config.thread_count,
                    consensus_config.t0,
                    consensus_config.genesis_timestamp,
                    header.content.slot,
                )?;
                if start <= time && time < end {
                    res.push(BlockSummary {
                        id,
                        is_final: status == Status::Final,
                        is_stale: false, // TODO: in the old api we considered only active blocks. Do we need to consider also discarded blocks?
                        is_in_blockclique: block_clique.block_ids.contains(&id),
                        slot: header.content.slot,
                        creator: Address::from_public_key(&header.content.creator)?,
                        parents: header.content.parents,
                    })
                }
            }
            let (start_slot, end_slot) = time_range_to_slot_range(
                consensus_config.thread_count,
                consensus_config.t0,
                consensus_config.genesis_timestamp,
                time.start,
                time.end,
            )?;
            if let Some(opt_storage_command_sender) = opt_storage_command_sender {
                let blocks = opt_storage_command_sender
                    .get_slot_range(start_slot, end_slot)
                    .await?;
                for (id, block) in blocks {
                    res.push(BlockSummary {
                        id,
                        is_final: true,
                        is_stale: false,         // because in storage
                        is_in_blockclique: true, // because in storage
                        slot: block.header.content.slot,
                        creator: Address::from_public_key(&block.header.content.creator)?,
                        parents: block.header.content.parents,
                    })
                }
            }
            Ok(res)
        };
        Box::pin(closure())
    }

    fn get_addresses(
        &self,
        addresses: Vec<Address>,
    ) -> BoxFuture<Result<Vec<AddressInfo>, ApiError>> {
        let cmd_sender = self.0.consensus_command_sender.clone();
        let storage_cmd_sender = self.0.storage_command_sender.clone();
        let cfg = self.0.consensus_config.clone();
        let api_cfg = self.0.api_config;
        let addrs = addresses;
        let mut pool_command_sender = self.0.pool_command_sender.clone();
        let compensation_millis = self.0.compensation_millis;
        let closure = async move || {
            let mut res = Vec::new();
            // roll and balance info
            let cloned = addrs.clone();
            let states = cmd_sender
                .get_addresses_info(cloned.into_iter().collect())
                .await?;
            // next draws info
            let now = UTime::now(compensation_millis)?;
            let current_slot = get_latest_block_slot_at_timestamp(
                cfg.thread_count,
                cfg.t0,
                cfg.genesis_timestamp,
                now,
            )?
            .unwrap_or_else(|| Slot::new(0, 0));
            let next_draws = cmd_sender
                .get_selection_draws(
                    current_slot,
                    Slot::new(
                        current_slot.period + api_cfg.draw_lookahead_period_count,
                        current_slot.thread,
                    ),
                )
                .await?;
            // block info
            let mut blocks = HashMap::new();
            let cloned = addrs.clone();
            for ad in cloned.iter() {
                blocks.insert(
                    ad,
                    cmd_sender
                        .get_block_ids_by_creator(*ad)
                        .await?
                        .into_keys()
                        .collect::<BlockHashSet>(),
                );
                if let Some(storage_cmd_sender) = storage_cmd_sender.clone() {
                    let new = storage_cmd_sender.get_block_ids_by_creator(ad).await?;
                    match blocks.entry(ad) {
                        Entry::Occupied(mut occ) => occ.get_mut().extend(new),
                        Entry::Vacant(vac) => {
                            vac.insert(new);
                        }
                    }
                }
            }
            // endorsements info
            // TODO: add get_endorsements_by_address consensus command -> wait for !238

            // operations info
            let mut ops = HashMap::new();
            let cloned = addrs.clone();
            for ad in cloned.iter() {
                let mut res: OperationHashMap<_> = pool_command_sender
                    .get_operations_involving_address(*ad)
                    .await?;
                cmd_sender
                    .get_operations_involving_address(*ad)
                    .await?
                    .into_iter()
                    .for_each(|(op_id, search_new)| {
                        res.entry(op_id)
                            .and_modify(|search_old| search_old.extend(&search_new))
                            .or_insert(search_new);
                    });
                if let Some(storage_cmd_sender) = storage_cmd_sender.clone() {
                    storage_cmd_sender
                        .get_operations_involving_address(&ad)
                        .await?
                        .into_iter()
                        .for_each(|(op_id, search_new)| {
                            res.entry(op_id)
                                .and_modify(|search_old| search_old.extend(&search_new))
                                .or_insert(search_new);
                        });
                }
                ops.insert(ad, res);
            }
            // staking addrs
            let staking_addrs = cmd_sender.get_staking_addresses().await?;
            for address in addrs.into_iter() {
                let state = states.get(&address).ok_or(ApiError::NotFound)?;
                res.push(AddressInfo {
                    address,
                    thread: address.get_thread(cfg.thread_count),
                    balance: BalanceInfo {
                        final_balance: state.final_ledger_data.balance,
                        candidate_balance: state.candidate_ledger_data.balance,
                        locked_balance: state.locked_balance,
                    },
                    rolls: RollsInfo {
                        active_rolls: state.active_rolls.unwrap_or_default(),
                        final_rolls: state.final_rolls,
                        candidate_rolls: state.candidate_rolls,
                    },
                    block_draws: next_draws
                        .iter()
                        .filter(|(_, (ad, _))| *ad == address)
                        .map(|(slot, _)| *slot)
                        .collect(),
                    endorsement_draws: next_draws
                        .iter()
                        .filter(|(_, (_, ads))| ads.contains(&address))
                        .map(|(slot, (_, ads))| {
                            ads.iter()
                                .enumerate()
                                .filter(|(_, ad)| **ad == address)
                                .map(|(i, _)| (slot.to_string(), i as u64))
                                .collect::<Vec<(String, u64)>>()
                        })
                        .flatten()
                        .collect(),
                    blocks_created: blocks
                        .get(&address)
                        .ok_or(ApiError::NotFound)?
                        .into_iter()
                        .copied()
                        .collect(),
                    involved_in_endorsements: HashSet::new().into_iter().collect(), // TODO: update wait for !238
                    involved_in_operations: ops
                        .get(&address)
                        .ok_or(ApiError::NotFound)?
                        .keys()
                        .copied()
                        .collect(),
                    is_staking: staking_addrs.contains(&address),
                })
            }
            Ok(res)
        };
        Box::pin(closure())
    }

    fn send_operations(
        &self,
        ops: Vec<Operation>,
    ) -> BoxFuture<Result<Vec<OperationId>, ApiError>> {
        let mut cmd_sender = self.0.pool_command_sender.clone();
        let closure = async move || {
            let to_send = ops
                .into_iter()
                .map(|op| Ok((op.verify_integrity()?, op)))
                .collect::<Result<OperationHashMap<_>, ApiError>>()?;
            let ids = to_send.keys().copied().collect();
            cmd_sender.add_operations(to_send).await?;
            Ok(ids)
        };
        Box::pin(closure())
    }
}
