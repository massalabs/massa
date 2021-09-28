// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(async_closure)]
use api_dto::{
    AddressInfo, BalanceInfo, BlockInfo, BlockSummary, EndorsementInfo, NodeStatus, OperationInfo,
    RollsInfo,
};
use consensus::{
    get_block_slot_timestamp, get_latest_block_slot_at_timestamp, time_range_to_slot_range,
    ConsensusCommandSender, ConsensusConfig, Status,
};
use error::PublicApiError;
use jsonrpc_core::{BoxFuture, IoHandler};
use jsonrpc_derive::rpc;
use jsonrpc_http_server::ServerBuilder;
use models::address::AddressHashMap;
use models::clique::Clique;
use models::operation::{Operation, OperationId};
use models::EndorsementId;
use models::{Address, BlockId, Slot};
use pool::PoolCommandSender;
use rpc_server::APIConfig;
pub use rpc_server::API;
use std::collections::{HashMap, HashSet};
use std::thread;
use storage::StorageAccess;
use time::UTime;

mod error;

pub struct ApiMassaPublic {
    pub url: String,
    pub consensus_command_sender: ConsensusCommandSender,
    pub pool_command_sender: PoolCommandSender,
    pub storage_command_sender: Option<StorageAccess>,
    pub consensus_config: ConsensusConfig,
    pub api_config: APIConfig,
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
    ) -> Self {
        ApiMassaPublic {
            url: url.to_string(),
            consensus_command_sender,
            consensus_config,
            api_config,
            pool_command_sender,
            storage_command_sender,
        }
    }

    /// Starts massa public server.
    pub fn serve_massa_public(self) {
        let mut io = IoHandler::new();
        let url = self.url.parse().unwrap();
        io.extend_with(self.to_delegate());

        let server = ServerBuilder::new(io)
            .start_http(&url)
            .expect("Unable to start RPC server");

        thread::spawn(|| server.wait());
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
    fn get_status(&self) -> jsonrpc_core::Result<NodeStatus>;

    #[rpc(name = "get_cliques")]
    fn get_cliques(&self) -> BoxFuture<Result<Vec<Clique>, PublicApiError>>;

    //////////////////////////////////
    // Debug (specific information) //
    //////////////////////////////////

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
    #[rpc(name = "get_block")]
    fn get_block(&self, _: BlockId) -> BoxFuture<Result<BlockInfo, PublicApiError>>;

    /// Get the block graph within the specified time interval.
    /// Optional parameters: from <time_start> (included) and to <time_end> (excluded) millisecond timestamp
    #[rpc(name = "get_graph_interval")]
    fn get_graph_interval(
        &self,
        time_start: Option<UTime>,
        time_end: Option<UTime>,
    ) -> BoxFuture<Result<Vec<BlockSummary>, PublicApiError>>;

    #[rpc(name = "get_addresses")]
    fn get_addresses(&self, _: Vec<Address>)
        -> BoxFuture<Result<Vec<AddressInfo>, PublicApiError>>;

    //////////////////////////////////////
    // User (interaction with the node) //
    //////////////////////////////////////

    /// Adds operations to pool.
    #[rpc(name = "send_operations")]
    fn send_operations(&self, _: Vec<Operation>) -> BoxFuture<Result<(), PublicApiError>>;
}

impl MassaPublic for ApiMassaPublic {
    fn get_status(&self) -> jsonrpc_core::Result<NodeStatus> {
        todo!()
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
        // todo use that command sender
        let opt_storage_command_sender = self.storage_command_sender.clone();
        let closure = async move || {
            Ok(consensus_command_sender
                .get_operations(ops.into_iter().collect())
                .await?
                .into_iter()
                .map(|(id, op)| OperationInfo {
                    id,
                    in_pool: op.in_pool,
                    in_blocks: op.in_blocks.keys().copied().collect(),
                    is_final: op.in_blocks.iter().any(|(_, (_, is_final))| *is_final),
                    operation: op.op,
                })
                .collect())
        };

        Box::pin(closure())
    }

    fn get_endorsements(
        &self,
        _: Vec<EndorsementId>,
    ) -> jsonrpc_core::Result<Vec<EndorsementInfo>> {
        todo!()
    }

    fn get_block(&self, id: BlockId) -> BoxFuture<Result<BlockInfo, PublicApiError>> {
        let consensus_command_sender = self.consensus_command_sender.clone();
        let opt_storage_command_sender = self.storage_command_sender.clone();
        let closure = async move || {
            let graph = consensus_command_sender.get_block_graph_status().await?;
            let block_clique = graph
                .max_cliques
                .iter()
                .find(|clique| clique.is_blockclique)
                .ok_or(PublicApiError::InconsistencyError(
                    "Missing block clique".to_string(),
                ))?;
            if let Some(block) = consensus_command_sender.get_active_block(id).await? {
                Ok(BlockInfo {
                    id,
                    is_final: false, // todo retrive block finality
                    is_stale: false,
                    is_in_blockclique: block_clique.block_ids.contains(&id),
                    block,
                })
            } else {
                if let Some(cmd_tx) = opt_storage_command_sender {
                    match cmd_tx.get_block(id).await {
                        Ok(Some(block)) => Ok(BlockInfo {
                            id,
                            is_final: true,
                            is_stale: false,
                            is_in_blockclique: block_clique.block_ids.contains(&id),
                            block,
                        }),
                        Ok(None) => Err(PublicApiError::NotFound),
                        Err(e) => Err(e.into()),
                    }
                } else {
                    Err(PublicApiError::NotFound)
                }
            }
        };

        Box::pin(closure())
    }

    fn get_graph_interval(
        &self,
        time_start: Option<UTime>,
        time_end: Option<UTime>,
    ) -> BoxFuture<Result<Vec<BlockSummary>, PublicApiError>> {
        let consensus_command_sender = self.consensus_command_sender.clone();
        let opt_storage_command_sender = self.storage_command_sender.clone();
        let consensus_config = self.consensus_config.clone();
        let closure = async move || {
            let graph = consensus_command_sender.get_block_graph_status().await?;

            //filter block from graph_export
            let start = time_start.unwrap_or_else(|| UTime::from(0));
            let end = time_end.unwrap_or_else(|| UTime::from(u64::MAX));
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
                        is_stale: false, // todo in the old api we considered only active blocks. Do we need to consider also discarded blocks ?
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
                    time_start,
                    time_end,
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

    fn send_operations(&self, ops: Vec<Operation>) -> BoxFuture<Result<(), PublicApiError>> {
        let mut cmd_sender = self.pool_command_sender.clone();
        let closure = async move || {
            Ok(cmd_sender
                .add_operations(
                    ops.into_iter()
                        .map(|op| Ok((op.get_operation_id()?, op)))
                        .collect::<Result<_, PublicApiError>>()?,
                )
                .await?)
        };
        Box::pin(closure())
    }

    fn get_addresses(
        &self,
        addresses: Vec<Address>,
    ) -> BoxFuture<Result<Vec<AddressInfo>, PublicApiError>> {
        let cmd_sender = self.consensus_command_sender.clone();
        let cfg = self.consensus_config.clone();
        let api_cfg = self.api_config.clone();
        let addrs = addresses.clone();
        let closure = async move || {
            let mut res = Vec::new();

            // roll and balance info

            let cloned = addrs.clone();
            let states = cmd_sender
                .get_addresses_info(cloned.into_iter().collect())
                .await?;

            // next draws info
            let now = UTime::now(0)?; // todo get clock compensation ?

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
                blocks.insert(ad, cmd_sender.get_block_ids_by_creator(*ad).await?);
            }

            // endorsements info
            // todo add get_endorsements_by_address consensus command

            // operations info
            let mut ops = HashMap::new();
            let cloned = addrs.clone();
            for ad in cloned.iter() {
                ops.insert(ad, cmd_sender.get_operations_involving_address(*ad).await?);
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
                        .keys()
                        .copied()
                        .collect(),
                    involved_in_endorsements: HashSet::new().into_iter().collect(), // todo update
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
}
