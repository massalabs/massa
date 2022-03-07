// Copyright (c) 2022 MASSA LABS <info@massa.net>
#![allow(clippy::too_many_arguments)]
use crate::error::ApiError;
use crate::{Endpoints, Public, RpcServer, StopHandle, API};
use futures::{stream::FuturesUnordered, StreamExt};
use jsonrpc_core::BoxFuture;
use massa_consensus_exports::{ConsensusCommandSender, ConsensusConfig};
use massa_execution_exports::{
    ExecutionController, ExecutionStackElement, ReadOnlyExecutionRequest,
};
use massa_graph::{DiscardReason, ExportBlockStatus};
use massa_models::api::SCELedgerInfo;
use massa_models::execution::ReadOnlyResult;
use massa_models::SignedOperation;

use massa_models::{
    api::{
        APISettings, AddressInfo, BlockInfo, BlockInfoContent, BlockSummary, EndorsementInfo,
        EventFilter, IndexedSlot, NodeStatus, OperationInfo, ReadOnlyExecution, TimeInterval,
    },
    clique::Clique,
    composite::PubkeySig,
    execution::ExecuteReadOnlyResponse,
    node::NodeId,
    output_event::SCOutputEvent,
    prehash::{BuildMap, Map, Set},
    timeslots::{get_latest_block_slot_at_timestamp, time_range_to_slot_range},
    Address, BlockId, CompactConfig, EndorsementId, OperationId, Slot, Version,
};
use massa_network::{NetworkCommandSender, NetworkSettings};
use massa_pool::PoolCommandSender;
use massa_signature::{derive_public_key, generate_random_private_key, PrivateKey};
use massa_time::MassaTime;
use std::net::{IpAddr, SocketAddr};

impl API<Public> {
    pub fn new(
        consensus_command_sender: ConsensusCommandSender,
        execution_controller: Box<dyn ExecutionController>,
        api_settings: &'static APISettings,
        consensus_settings: ConsensusConfig,
        pool_command_sender: PoolCommandSender,
        network_settings: &'static NetworkSettings,
        version: Version,
        network_command_sender: NetworkCommandSender,
        compensation_millis: i64,
        node_id: NodeId,
    ) -> Self {
        API(Public {
            consensus_command_sender,
            consensus_config: consensus_settings,
            api_settings,
            pool_command_sender,
            network_settings,
            version,
            network_command_sender,
            compensation_millis,
            node_id,
            execution_controller,
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
        crate::wrong_api::<()>()
    }

    fn node_sign_message(&self, _: Vec<u8>) -> BoxFuture<Result<PubkeySig, ApiError>> {
        crate::wrong_api::<PubkeySig>()
    }

    fn add_staking_private_keys(&self, _: Vec<PrivateKey>) -> BoxFuture<Result<(), ApiError>> {
        crate::wrong_api::<()>()
    }

    fn execute_read_only_request(
        &self,
        reqs: Vec<ReadOnlyExecution>,
    ) -> BoxFuture<Result<Vec<ExecuteReadOnlyResponse>, ApiError>> {
        if reqs.len() > self.0.api_settings.max_arguments as usize {
            let closure =
                async move || Err(ApiError::TooManyArguments("too many arguments".into()));
            return Box::pin(closure());
        }

        let mut res: Vec<ExecuteReadOnlyResponse> = Vec::with_capacity(reqs.len());
        for ReadOnlyExecution {
            max_gas,
            address,
            simulated_gas_price,
            bytecode,
        } in reqs
        {
            let address = address.unwrap_or_else(|| {
                // if no addr provided, use a random one
                Address::from_public_key(&derive_public_key(&generate_random_private_key()))
            });

            // TODO:
            // * set a maximum gas value for read-only executions to prevent attacks
            // * stop mapping request and result, reuse execution's structures
            // * remove async stuff

            // translate request
            let req = ReadOnlyExecutionRequest {
                max_gas,
                simulated_gas_price,
                bytecode,
                call_stack: vec![ExecutionStackElement {
                    address,
                    coins: Default::default(),
                    owned_addresses: vec![address],
                }],
            };

            // run
            let result = self.0.execution_controller.execute_readonly_request(req);

            // map result
            let result = ExecuteReadOnlyResponse {
                executed_at: result.as_ref().map_or_else(|_| Slot::new(0, 0), |v| v.slot),
                result: result.as_ref().map_or_else(
                    |err| ReadOnlyResult::Error(format!("readonly call failed: {}", err)),
                    |_| ReadOnlyResult::Ok,
                ),
                output_events: result.map_or_else(|_| Default::default(), |v| v.events.export()),
            };

            res.push(result);
        }

        // return result
        let closure = async move || Ok(res);
        Box::pin(closure())
    }

    fn remove_staking_addresses(&self, _: Vec<Address>) -> BoxFuture<Result<(), ApiError>> {
        crate::wrong_api::<()>()
    }

    fn get_staking_addresses(&self) -> BoxFuture<Result<Set<Address>, ApiError>> {
        crate::wrong_api::<Set<Address>>()
    }

    fn ban(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        crate::wrong_api::<()>()
    }

    fn unban(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        crate::wrong_api::<()>()
    }

    fn get_status(&self) -> BoxFuture<Result<NodeStatus, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let network_command_sender = self.0.network_command_sender.clone();
        let network_config = self.0.network_settings.clone();
        let version = self.0.version;
        let consensus_settings = self.0.consensus_config.clone();
        let compensation_millis = self.0.compensation_millis;
        let mut pool_command_sender = self.0.pool_command_sender.clone();
        let node_id = self.0.node_id;
        let config = CompactConfig::default();
        let closure = async move || {
            let now = MassaTime::compensated_now(compensation_millis)?;
            let last_slot = get_latest_block_slot_at_timestamp(
                consensus_settings.thread_count,
                consensus_settings.t0,
                consensus_settings.genesis_timestamp,
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
                current_time: now,
                connected_nodes: peers?
                    .peers
                    .iter()
                    .flat_map(|(ip, peer)| peer.active_nodes.iter().map(move |(id, _)| (*id, *ip)))
                    .collect(),
                last_slot,
                next_slot: last_slot
                    .unwrap_or_else(|| Slot::new(0, 0))
                    .get_next_slot(consensus_settings.thread_count)?,
                consensus_stats: consensus_stats?,
                network_stats: network_stats?,
                pool_stats: pool_stats?,

                config,
                current_cycle: last_slot
                    .unwrap_or_else(|| Slot::new(0, 0))
                    .get_cycle(consensus_settings.periods_per_cycle),
            })
        };
        Box::pin(closure())
    }

    fn get_cliques(&self) -> BoxFuture<Result<Vec<Clique>, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let closure = async move || Ok(consensus_command_sender.get_cliques().await?);
        Box::pin(closure())
    }

    fn get_stakers(&self) -> BoxFuture<Result<Map<Address, u64>, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let closure = async move || Ok(consensus_command_sender.get_active_stakers().await?);
        Box::pin(closure())
    }

    fn get_operations(
        &self,
        ops: Vec<OperationId>,
    ) -> BoxFuture<Result<Vec<OperationInfo>, ApiError>> {
        let api_cfg = self.0.api_settings;
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let mut pool_command_sender = self.0.pool_command_sender.clone();
        let closure = async move || {
            if ops.len() as u64 > api_cfg.max_arguments {
                return Err(ApiError::TooManyArguments("too many arguments".into()));
            }

            let operation_ids: Set<OperationId> = ops.iter().cloned().collect();

            // simultaneously ask pool and consensus
            let (pool_res, consensus_res) = tokio::join!(
                pool_command_sender.get_operations(operation_ids.clone()),
                consensus_command_sender.get_operations(operation_ids)
            );
            let (pool_res, consensus_res) = (pool_res?, consensus_res?);
            let mut res: Map<OperationId, OperationInfo> = Map::with_capacity_and_hasher(
                pool_res.len() + consensus_res.len(),
                BuildMap::default(),
            );

            // add pool info
            res.extend(pool_res.into_iter().map(|(id, operation)| {
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
            }));

            // add consensus info
            consensus_res.into_iter().for_each(|(op_id, search_new)| {
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

            // return values in the right order
            Ok(ops
                .into_iter()
                .filter_map(|op_id| res.remove(&op_id))
                .collect())
        };
        Box::pin(closure())
    }

    fn get_endorsements(
        &self,
        eds: Vec<EndorsementId>,
    ) -> BoxFuture<Result<Vec<EndorsementInfo>, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let pool_command_sender = self.0.pool_command_sender.clone();
        let closure = async move || {
            let mapped: Set<EndorsementId> = eds.into_iter().collect();
            let mut res = consensus_command_sender
                .get_endorsements_by_id(mapped.clone())
                .await?;
            for (id, endorsement) in pool_command_sender.get_endorsements_by_id(mapped).await? {
                res.entry(id)
                    .and_modify(|EndorsementInfo { in_pool, .. }| *in_pool = true)
                    .or_insert(EndorsementInfo {
                        id,
                        in_pool: true,
                        in_blocks: vec![],
                        is_final: false,
                        endorsement,
                    });
            }
            Ok(res.values().cloned().collect())
        };
        Box::pin(closure())
    }

    /// gets a block. Returns None if not found
    /// only active blocks are returned
    fn get_block(&self, id: BlockId) -> BoxFuture<Result<BlockInfo, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let closure = async move || {
            let cliques = consensus_command_sender.get_cliques().await?;
            let blockclique = cliques
                .iter()
                .find(|clique| clique.is_blockclique)
                .ok_or_else(|| ApiError::InconsistencyError("Missing block clique".to_string()))?;

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
                Ok(BlockInfo {
                    id,
                    content: Some(BlockInfoContent {
                        is_final,
                        is_stale: false,
                        is_in_blockclique: blockclique.block_ids.contains(&id),
                        block,
                    }),
                })
            } else {
                Ok(BlockInfo { id, content: None })
            }
        };
        Box::pin(closure())
    }

    /// gets an interval of the block graph from consensus, with time filtering
    /// time filtering is done consensus-side to prevent communication overhead
    fn get_graph_interval(
        &self,
        time: TimeInterval,
    ) -> BoxFuture<Result<Vec<BlockSummary>, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let consensus_settings = self.0.consensus_config.clone();
        let closure = async move || {
            // filter blocks from graph_export
            let (start_slot, end_slot) = time_range_to_slot_range(
                consensus_settings.thread_count,
                consensus_settings.t0,
                consensus_settings.genesis_timestamp,
                time.start,
                time.end,
            )?;
            let graph = consensus_command_sender
                .get_block_graph_status(start_slot, end_slot)
                .await?;
            let mut res = Vec::with_capacity(graph.active_blocks.len());
            let blockclique = graph
                .max_cliques
                .iter()
                .find(|clique| clique.is_blockclique)
                .ok_or_else(|| ApiError::InconsistencyError("missing blockclique".to_string()))?;
            for (id, exported_block) in graph.active_blocks.into_iter() {
                res.push(BlockSummary {
                    id,
                    is_final: exported_block.is_final,
                    is_stale: false,
                    is_in_blockclique: blockclique.block_ids.contains(&id),
                    slot: exported_block.header.content.slot,
                    creator: Address::from_public_key(&exported_block.header.content.creator),
                    parents: exported_block.header.content.parents,
                });
            }
            for (id, (reason, header)) in graph.discarded_blocks.into_iter() {
                if reason == DiscardReason::Stale {
                    res.push(BlockSummary {
                        id,
                        is_final: false,
                        is_stale: true,
                        is_in_blockclique: false,
                        slot: header.content.slot,
                        creator: Address::from_public_key(&header.content.creator),
                        parents: header.content.parents,
                    });
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
        let cfg = self.0.consensus_config.clone();
        let api_cfg = self.0.api_settings;
        let pool_command_sender = self.0.pool_command_sender.clone();
        let compensation_millis = self.0.compensation_millis;

        // todo make better use of SCE ledger info

        // map SCE ledger info and check for address length
        let sce_ledger_info = if addresses.len() as u64 > api_cfg.max_arguments {
            Err(ApiError::TooManyArguments("too many arguments".into()))
        } else {
            // get SCE ledger info
            let mut sce_ledger_info: Map<Address, SCELedgerInfo> =
                Map::with_capacity_and_hasher(addresses.len(), BuildMap::default());
            for addr in &addresses {
                let active_entry = match self
                    .0
                    .execution_controller
                    .get_final_and_active_ledger_entry(addr)
                    .1
                {
                    None => continue,
                    Some(v) => SCELedgerInfo {
                        balance: v.parallel_balance,
                        module: Some(v.bytecode),
                        datastore: v.datastore.into_iter().collect(),
                    },
                };
                sce_ledger_info.insert(*addr, active_entry);
            }
            Ok(sce_ledger_info)
        };

        let closure = async move || {
            let sce_ledger_info = sce_ledger_info?;

            let mut res = Vec::with_capacity(addresses.len());

            // next draws info
            let now = MassaTime::compensated_now(compensation_millis)?;
            let current_slot = get_latest_block_slot_at_timestamp(
                cfg.thread_count,
                cfg.t0,
                cfg.genesis_timestamp,
                now,
            )?
            .unwrap_or_else(|| Slot::new(0, 0));
            let next_draws = cmd_sender.get_selection_draws(
                current_slot,
                Slot::new(
                    current_slot.period + api_cfg.draw_lookahead_period_count,
                    current_slot.thread,
                ),
            );

            // roll and balance info
            let states = cmd_sender.get_addresses_info(addresses.iter().copied().collect());

            // wait for both simultaneously
            let (next_draws, states) = tokio::join!(next_draws, states);
            let (next_draws, mut states) = (next_draws?, states?);

            // operations block and endorsement info
            let mut operations: Map<Address, Set<OperationId>> =
                Map::with_capacity_and_hasher(addresses.len(), BuildMap::default());
            let mut blocks: Map<Address, Set<BlockId>> =
                Map::with_capacity_and_hasher(addresses.len(), BuildMap::default());
            let mut endorsements: Map<Address, Set<EndorsementId>> =
                Map::with_capacity_and_hasher(addresses.len(), BuildMap::default());

            let mut concurrent_getters = FuturesUnordered::new();
            for &address in addresses.iter() {
                let mut pool_cmd_snd = pool_command_sender.clone();
                let cmd_snd = cmd_sender.clone();
                concurrent_getters.push(async move {
                    let blocks = cmd_snd
                        .get_block_ids_by_creator(address)
                        .await?
                        .into_keys()
                        .collect::<Set<BlockId>>();
                    let get_pool_ops = pool_cmd_snd.get_operations_involving_address(address);
                    let get_consensus_ops = cmd_snd.get_operations_involving_address(address);
                    let (get_pool_ops, get_consensus_ops) =
                        tokio::join!(get_pool_ops, get_consensus_ops);
                    let gathered: Set<OperationId> = get_pool_ops?
                        .into_keys()
                        .chain(get_consensus_ops?.into_keys())
                        .collect();

                        let get_pool_eds = pool_cmd_snd.get_endorsements_by_address(address);
                        let get_consensus_eds = cmd_snd.get_endorsements_by_address(address);
                        let (get_pool_eds, get_consensus_eds) =
                            tokio::join!(get_pool_eds, get_consensus_eds);
                        let gathered_ed: Set<EndorsementId> = get_pool_eds?
                            .into_keys()
                            .chain(get_consensus_eds?.into_keys())
                            .collect();
                    Result::<(Address, Set<BlockId>, Set<OperationId>, Set<EndorsementId>), ApiError>::Ok((
                        address, blocks, gathered, gathered_ed
                    ))
                });
            }
            while let Some(res) = concurrent_getters.next().await {
                let (a, bl_set, op_set, ed_set) = res?;
                operations.insert(a, op_set);
                blocks.insert(a, bl_set);
                endorsements.insert(a, ed_set);
            }

            // compile everything per address
            for address in addresses.into_iter() {
                let state = states.remove(&address).ok_or(ApiError::NotFound)?;
                res.push(AddressInfo {
                    address,
                    thread: address.get_thread(cfg.thread_count),
                    ledger_info: state.ledger_info,
                    rolls: state.rolls,
                    block_draws: next_draws
                        .iter()
                        .filter(|(_, (ad, _))| *ad == address)
                        .map(|(slot, _)| *slot)
                        .collect(),
                    endorsement_draws: next_draws
                        .iter()
                        .flat_map(|(slot, (_, addrs))| {
                            addrs.iter().enumerate().filter_map(|(index, ad)| {
                                if *ad == address {
                                    Some(IndexedSlot { slot: *slot, index })
                                } else {
                                    None
                                }
                            })
                        })
                        .collect(),
                    blocks_created: blocks.remove(&address).ok_or(ApiError::NotFound)?,
                    involved_in_endorsements: endorsements
                        .remove(&address)
                        .ok_or(ApiError::NotFound)?,
                    involved_in_operations: operations
                        .remove(&address)
                        .ok_or(ApiError::NotFound)?,
                    production_stats: state.production_stats,
                    sce_ledger_info: sce_ledger_info.get(&address).cloned().unwrap_or_default(),
                })
            }
            Ok(res)
        };
        Box::pin(closure())
    }

    fn send_operations(
        &self,
        ops: Vec<SignedOperation>,
    ) -> BoxFuture<Result<Vec<OperationId>, ApiError>> {
        let mut cmd_sender = self.0.pool_command_sender.clone();
        let api_cfg = self.0.api_settings;
        let closure = async move || {
            if ops.len() as u64 > api_cfg.max_arguments {
                return Err(ApiError::TooManyArguments("too many arguments".into()));
            }
            let to_send = ops
                .into_iter()
                .map(|op| Ok((op.verify_integrity()?, op)))
                .collect::<Result<Map<OperationId, _>, ApiError>>()?;
            let ids = to_send.keys().copied().collect();
            cmd_sender.add_operations(to_send).await?;
            Ok(ids)
        };
        Box::pin(closure())
    }

    /// Get events optionnally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    fn get_filtered_sc_output_event(
        &self,
        EventFilter {
            start,
            end,
            emitter_address,
            original_caller_address,
            original_operation_id,
        }: EventFilter,
    ) -> BoxFuture<Result<Vec<SCOutputEvent>, ApiError>> {
        // get events
        let events = self.0.execution_controller.get_filtered_sc_output_event(
            start,
            end,
            emitter_address,
            original_caller_address,
            original_operation_id,
        );

        // TODO get rid of the async part
        let closure = async move || Ok(events);
        Box::pin(closure())
    }
}
