// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(async_closure)]

use crate::error::PrivateApiError;
use consensus::ExportBlockStatus;
use consensus::{ConsensusCommandSender, ConsensusConfig, Status};
use crypto::signature::PrivateKey;
use error::PublicApiError;
use jsonrpc_core::{BoxFuture, IoHandler};
use jsonrpc_derive::rpc;
use jsonrpc_http_server::CloseHandle;
use jsonrpc_http_server::ServerBuilder;
use models::address::{AddressHashMap, AddressHashSet};
use models::api::{
    APIConfig, AddressInfo, BalanceInfo, BlockInfo, BlockInfoContent, BlockSummary,
    EndorsementInfo, NodeStatus, OperationInfo, RollsInfo, TimeInterval,
};
use models::clique::Clique;
use models::crypto::PubkeySig;
use models::node::NodeId;
use models::operation::{Operation, OperationId};
use models::timeslots::get_block_slot_timestamp;
use models::timeslots::get_latest_block_slot_at_timestamp;
use models::timeslots::time_range_to_slot_range;
use models::OperationHashMap;
use models::OperationHashSet;
use models::{Address, BlockId, Slot};
use models::{AlgoConfig, BlockHashSet};
use models::{EndorsementId, Version};
use network::NetworkCommandSender;
use network::NetworkConfig;
use pool::PoolCommandSender;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::thread;
use std::thread::JoinHandle;
use storage::StorageAccess;
use time::UTime;
use tokio::sync::mpsc;
use tracing::{info, warn};

mod error;

pub struct ApiMassaPublicStopHandle {
    close_handle: CloseHandle,
    join_handle: JoinHandle<()>,
}

impl ApiMassaPublicStopHandle {
    pub fn stop(self) {
        self.close_handle.close();
        if let Err(err) = self.join_handle.join() {
            warn!("public API thread panicked: {:?}", err);
        } else {
            info!("public API finished cleanly");
        }
    }
}

pub struct ApiMassaPublic {
    pub url: String,
    pub consensus_command_sender: ConsensusCommandSender,
    pub pool_command_sender: PoolCommandSender,
    pub storage_command_sender: Option<StorageAccess>,
    pub consensus_config: ConsensusConfig,
    pub api_config: APIConfig,
    pub network_config: NetworkConfig,
    pub version: Version,
    pub network_command_sender: NetworkCommandSender,
    pub compensation_millis: i64,
    pub node_id: NodeId,
}

impl ApiMassaPublic {
    /// creates Private Api from url and need command senders and configs
    pub fn create(
        url: &str,
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
        ApiMassaPublic {
            url: url.to_string(),
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
        }
    }

    /// Starts massa public server.
    pub fn serve_massa_public(self) -> ApiMassaPublicStopHandle {
        let mut io = IoHandler::new();
        let url = self.url.parse().unwrap();
        io.extend_with(self.to_delegate());

        let server = ServerBuilder::new(io)
            .event_loop_executor(tokio::runtime::Handle::current())
            .start_http(&url)
            .expect("Unable to start RPC server");

        let close_handle = server.close_handle();
        let join_handle = thread::spawn(|| server.wait());

        ApiMassaPublicStopHandle {
            close_handle,
            join_handle,
        }
    }
}

/// Public Massa JSON-RPC endpoints
#[rpc(server)]
pub trait MassaPublic {
    /////////////////////////////////
    // Explorer (aggregated stats) //
    /////////////////////////////////

    /// summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count
    #[rpc(name = "get_status")]
    fn get_status(&self) -> BoxFuture<Result<NodeStatus, PublicApiError>>;

    #[rpc(name = "get_cliques")]
    fn get_cliques(&self) -> BoxFuture<Result<Vec<Clique>, PublicApiError>>;

    //////////////////////////////////
    // Debug (specific information) //
    //////////////////////////////////

    /// Returns the algo Config.
    #[rpc(name = "get_algo_config")]
    fn get_algo_config(&self) -> BoxFuture<Result<AlgoConfig, PublicApiError>>;

    /// Returns the compensation_millis.
    #[rpc(name = "get_compensation_millis")]
    fn get_compensation_millis(&self) -> BoxFuture<Result<i64, PublicApiError>>;

    /// Returns the active stakers and their roll counts for the current cycle.
    #[rpc(name = "get_stakers")]
    fn get_stakers(&self) -> BoxFuture<Result<AddressHashMap<RollsInfo>, PublicApiError>>;

    /// Returns operations information associated to a given list of operations' IDs.
    #[rpc(name = "get_operations")]
    fn get_operations(
        &self,
        _: Vec<OperationId>,
    ) -> BoxFuture<Result<Vec<OperationInfo>, PublicApiError>>;

    #[rpc(name = "get_endorsements")]
    fn get_endorsements(&self, _: Vec<EndorsementId>)
        -> jsonrpc_core::Result<Vec<EndorsementInfo>>;

    /// Get information on a block given its hash
    #[rpc(name = "get_blocks")]
    fn get_blocks(&self, _: Vec<BlockId>) -> BoxFuture<Result<Vec<BlockInfo>, PublicApiError>>;

    /// Get the block graph within the specified time interval.
    /// Optional parameters: from <time_start> (included) and to <time_end> (excluded) millisecond timestamp
    #[rpc(name = "get_graph_interval")]
    fn get_graph_interval(
        &self,
        _: TimeInterval,
    ) -> BoxFuture<Result<Vec<BlockSummary>, PublicApiError>>;

    #[rpc(name = "get_addresses")]
    fn get_addresses(&self, _: Vec<Address>)
        -> BoxFuture<Result<Vec<AddressInfo>, PublicApiError>>;

    //////////////////////////////////////
    // User (interaction with the node) //
    //////////////////////////////////////

    /// Adds operations to pool. Returns operations that were ok and sent to pool.
    #[rpc(name = "send_operations")]
    fn send_operations(
        &self,
        _: Vec<Operation>,
    ) -> BoxFuture<Result<Vec<OperationId>, PublicApiError>>;
}

impl MassaPublic for ApiMassaPublic {
    fn get_status(&self) -> BoxFuture<Result<NodeStatus, PublicApiError>> {
        let consensus_command_sender = self.consensus_command_sender.clone();
        let network_command_sender = self.network_command_sender.clone();
        let network_config = self.network_config.clone();
        let version = self.version.clone();
        let consensus_config = self.consensus_config.clone();
        let compensation_millis = self.compensation_millis;
        let mut pool_command_sender = self.pool_command_sender.clone();
        let node_id = self.node_id.clone();

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
                    .unwrap_or(Slot::new(0, 0))
                    .get_next_slot(consensus_config.thread_count)?,
                consensus_stats: consensus_stats?,
                network_stats: network_stats?,
                pool_stats: pool_stats?,
            })
        };

        Box::pin(closure())
    }

    fn get_cliques(&self) -> BoxFuture<Result<Vec<Clique>, PublicApiError>> {
        let consensus_command_sender = self.consensus_command_sender.clone();
        let closure = async move || {
            Ok(consensus_command_sender
                .get_block_graph_status()
                .await?
                .max_cliques)
        };

        Box::pin(closure())
    }

    fn get_stakers(&self) -> BoxFuture<Result<AddressHashMap<RollsInfo>, PublicApiError>> {
        let consensus_command_sender = self.consensus_command_sender.clone();
        let closure = async move || {
            let addrs = consensus_command_sender.get_staking_addresses().await?;
            let info = consensus_command_sender.get_addresses_info(addrs).await?;
            Ok(info
                .into_iter()
                .map(|(ad, state)| {
                    (
                        ad,
                        RollsInfo {
                            active_rolls: state.active_rolls.unwrap_or_default(),
                            final_rolls: state.final_rolls,
                            candidate_rolls: state.candidate_rolls,
                        },
                    )
                })
                .collect())
        };

        Box::pin(closure())
    }

    fn get_operations(
        &self,
        ops: Vec<OperationId>,
    ) -> BoxFuture<Result<Vec<OperationInfo>, PublicApiError>> {
        let consensus_command_sender = self.consensus_command_sender.clone();
        let mut pool_command_sender = self.pool_command_sender.clone();
        let storage_command_sender = self.storage_command_sender.clone();
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
            if let Some(storage) = storage_command_sender {
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
                storage
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
    ) -> jsonrpc_core::Result<Vec<EndorsementInfo>> {
        todo!() // TODO: wait for !238
    }

    fn get_blocks(&self, ids: Vec<BlockId>) -> BoxFuture<Result<Vec<BlockInfo>, PublicApiError>> {
        let consensus_command_sender = self.consensus_command_sender.clone();
        let opt_storage_command_sender = self.storage_command_sender.clone();
        let closure = async move || {
            let mut res = Vec::new();
            let graph = consensus_command_sender.get_block_graph_status().await?;
            let block_clique = graph
                .max_cliques
                .iter()
                .find(|clique| clique.is_blockclique)
                .ok_or(PublicApiError::InconsistencyError(
                    "Missing block clique".to_string(),
                ))?;
            for id in ids.into_iter() {
                if let Some((block, is_final)) =
                    match consensus_command_sender.get_block_status(id).await? {
                        Some(ExportBlockStatus::Active(block)) => Some((block, false)),
                        Some(ExportBlockStatus::Incoming) => None,
                        Some(ExportBlockStatus::WaitingForSlot) => None,
                        Some(ExportBlockStatus::WaitingForDependencies) => None,
                        Some(ExportBlockStatus::Discarded(_)) => None, // TODO: get block if stale
                        Some(ExportBlockStatus::Final(block)) => Some((block, true)),
                        Some(ExportBlockStatus::Stored(_)) => None, // TODO: remove with old api
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
                } else {
                    if let Some(ref cmd_tx) = opt_storage_command_sender {
                        match cmd_tx.get_block(id).await {
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
                    } else {
                        return Err(PublicApiError::NotFound);
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
    ) -> BoxFuture<Result<Vec<BlockSummary>, PublicApiError>> {
        let consensus_command_sender = self.consensus_command_sender.clone();
        let opt_storage_command_sender = self.storage_command_sender.clone();
        let consensus_config = self.consensus_config.clone();
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
                .ok_or(PublicApiError::InconsistencyError(
                    "Missing block clique".to_string(),
                ))?;
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

            if let Some(storage) = opt_storage_command_sender {
                let (start_slot, end_slot) = time_range_to_slot_range(
                    consensus_config.thread_count,
                    consensus_config.t0,
                    consensus_config.genesis_timestamp,
                    time.start,
                    time.end,
                )?;

                let blocks = storage.get_slot_range(start_slot, end_slot).await?;
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

    fn send_operations(
        &self,
        ops: Vec<Operation>,
    ) -> BoxFuture<Result<Vec<OperationId>, PublicApiError>> {
        let mut cmd_sender = self.pool_command_sender.clone();
        let closure = async move || {
            let to_send = ops
                .into_iter()
                .map(|op| Ok((op.verify_integrity()?, op)))
                .collect::<Result<OperationHashMap<_>, PublicApiError>>()?;
            let ids = to_send.keys().copied().collect();
            cmd_sender.add_operations(to_send).await?;
            Ok(ids)
        };
        Box::pin(closure())
    }

    fn get_addresses(
        &self,
        addresses: Vec<Address>,
    ) -> BoxFuture<Result<Vec<AddressInfo>, PublicApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let storage_cmd_sender = self.storage_command_sender.clone();
        let cfg = self.consensus_config.clone();
        let api_cfg = self.api_config.clone();
        let addrs = addresses.clone();
        let mut pool_command_sender = self.pool_command_sender.clone();
        let compensation_millis = self.compensation_millis;
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
            .unwrap_or(Slot::new(0, 0));

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
                if let Some(access) = &storage_cmd_sender {
                    let new = access.get_block_ids_by_creator(ad).await?;
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

                if let Some(access) = &storage_cmd_sender {
                    access
                        .get_operations_involving_address(&ad)
                        .await?
                        .into_iter()
                        .for_each(|(op_id, search_new)| {
                            res.entry(op_id)
                                .and_modify(|search_old| search_old.extend(&search_new))
                                .or_insert(search_new);
                        })
                }

                ops.insert(ad, res);
            }

            // staking addrs
            let staking_addrs = cmd_sender.get_staking_addresses().await?;

            for address in addrs.into_iter() {
                let state = states.get(&address).ok_or(PublicApiError::NotFound)?;
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
                                .map(|(i, _)| (*slot, i as u64))
                                .collect::<Vec<(Slot, u64)>>()
                        })
                        .flatten()
                        .collect(),
                    blocks_created: blocks
                        .get(&address)
                        .ok_or(PublicApiError::NotFound)?
                        .into_iter()
                        .copied()
                        .collect(),
                    involved_in_endorsements: HashSet::new().into_iter().collect(), // TODO: update wait for !238
                    involved_in_operations: ops
                        .get(&address)
                        .ok_or(PublicApiError::NotFound)?
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

    fn get_algo_config(&self) -> BoxFuture<Result<AlgoConfig, PublicApiError>> {
        let cfg = self.consensus_config.clone();
        let closure = async move || Ok(cfg.to_algo_config());
        Box::pin(closure())
    }

    fn get_compensation_millis(&self) -> BoxFuture<Result<i64, PublicApiError>> {
        let cfg = self.compensation_millis.clone();
        let closure = async move || Ok(cfg);
        Box::pin(closure())
    }
}

pub struct ApiMassaPrivateStopHandle {
    close_handle: CloseHandle,
    join_handle: JoinHandle<()>,
}

impl ApiMassaPrivateStopHandle {
    pub fn stop(self) {
        self.close_handle.close();
        if let Err(err) = self.join_handle.join() {
            warn!("private API thread panicked: {:?}", err);
        } else {
            info!("private API finished cleanly");
        }
    }
}

pub struct ApiMassaPrivate {
    pub url: String,
    pub consensus_command_sender: ConsensusCommandSender,
    pub network_command_sender: NetworkCommandSender,
    pub consensus_config: ConsensusConfig,
    pub api_config: APIConfig,
    pub stop_node_channel: mpsc::Sender<()>,
}

/// Private Massa-RPC "manager mode" endpoints
#[rpc(server)]
pub trait MassaPrivate {
    /// Gracefully stop the node.
    #[rpc(name = "stop_node")]
    fn stop_node(&self) -> BoxFuture<Result<(), PrivateApiError>>;

    /// Sign message with node's key.
    /// Returns the public key that signed the message and the signature.
    #[rpc(name = "node_sign_message")]
    fn node_sign_message(&self, _: Vec<u8>) -> BoxFuture<Result<PubkeySig, PrivateApiError>>;

    /// Add a vec of new private keys for the node to use to stake.
    /// No confirmation to expect.
    #[rpc(name = "add_staking_private_keys")]
    fn add_staking_private_keys(
        &self,
        _: Vec<PrivateKey>,
    ) -> BoxFuture<Result<(), PrivateApiError>>;

    /// Remove a vec of addresses used to stake.
    /// No confirmation to expect.
    #[rpc(name = "remove_staking_addresses")]
    fn remove_staking_addresses(&self, _: Vec<Address>) -> BoxFuture<Result<(), PrivateApiError>>;

    /// Return hashset of staking addresses.
    #[rpc(name = "get_staking_addresses")]
    fn get_staking_addresses(&self) -> BoxFuture<Result<AddressHashSet, PrivateApiError>>;

    /// Bans given node id
    /// No confirmation to expect.
    #[rpc(name = "ban")]
    fn ban(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), PrivateApiError>>;

    /// Unbans given ip addr
    /// No confirmation to expect.
    #[rpc(name = "unban")]
    fn unban(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), PrivateApiError>>;
}

impl ApiMassaPrivate {
    /// creates Private Api from url and need command senders and configs
    pub fn create(
        url: &str,
        consensus_command_sender: ConsensusCommandSender,
        network_command_sender: NetworkCommandSender,
        api_config: APIConfig,
        consensus_config: ConsensusConfig,
    ) -> (Self, mpsc::Receiver<()>) {
        let (stop_node_channel, rx) = mpsc::channel(1);
        (
            ApiMassaPrivate {
                url: url.to_string(),
                consensus_command_sender,
                network_command_sender,
                consensus_config,
                api_config,
                stop_node_channel: stop_node_channel,
            },
            rx,
        )
    }

    /// Starts massa private server.
    pub fn serve_massa_private(self) -> ApiMassaPrivateStopHandle {
        let mut io = IoHandler::new();
        let url = self.url.parse().unwrap();
        io.extend_with(self.to_delegate());

        let server = ServerBuilder::new(io)
            .event_loop_executor(tokio::runtime::Handle::current())
            .start_http(&url)
            .expect("Unable to start RPC server");

        let close_handle = server.close_handle();
        let join_handle = thread::spawn(|| server.wait());

        ApiMassaPrivateStopHandle {
            close_handle,
            join_handle,
        }
    }
}

impl MassaPrivate for ApiMassaPrivate {
    fn stop_node(&self) -> BoxFuture<Result<(), PrivateApiError>> {
        let stop = self.stop_node_channel.clone();
        let closure = async move || {
            stop.send(()).await.map_err(|e| {
                PrivateApiError::SendChannelError(format!("error sending stop signal {}", e))
            })?;
            Ok(())
        };

        Box::pin(closure())
    }

    fn node_sign_message(&self, message: Vec<u8>) -> BoxFuture<Result<PubkeySig, PrivateApiError>> {
        let network_command_sender = self.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.node_sign_message(message).await?);
        Box::pin(closure())
    }

    fn add_staking_private_keys(
        &self,
        keys: Vec<PrivateKey>,
    ) -> BoxFuture<Result<(), PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let closure = async move || Ok(cmd_sender.register_staking_private_keys(keys).await?);
        Box::pin(closure())
    }

    fn remove_staking_addresses(
        &self,
        keys: Vec<Address>,
    ) -> BoxFuture<Result<(), PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let closure = async move || {
            Ok(cmd_sender
                .remove_staking_addresses(keys.into_iter().collect())
                .await?)
        };
        Box::pin(closure())
    }

    fn get_staking_addresses(&self) -> BoxFuture<Result<AddressHashSet, PrivateApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let closure = async move || Ok(cmd_sender.get_staking_addresses().await?);
        Box::pin(closure())
    }

    fn ban(&self, ips: Vec<IpAddr>) -> BoxFuture<Result<(), PrivateApiError>> {
        let network_command_sender = self.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.ban_ip(ips).await?);
        Box::pin(closure())
    }

    fn unban(&self, ips: Vec<IpAddr>) -> BoxFuture<Result<(), PrivateApiError>> {
        let network_command_sender = self.network_command_sender.clone();
        let closure = async move || Ok(network_command_sender.unban(ips).await?);
        Box::pin(closure())
    }
}
