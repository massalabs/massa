//! Copyright (c) 2022 MASSA LABS <info@massa.net>
#![allow(clippy::too_many_arguments)]

use crate::config::APIConfig;
use crate::error::ApiError;
use crate::{Endpoints, Public, RpcServer, StopHandle, API};
use jsonrpc_core::BoxFuture;
use massa_consensus_exports::{ConsensusCommandSender, ConsensusConfig};
use massa_execution_exports::{
    ExecutionController, ExecutionStackElement, ReadOnlyExecutionRequest, ReadOnlyExecutionTarget,
};
use massa_graph::DiscardReason;
use massa_models::api::{
    BlockGraphStatus, DatastoreEntryInput, DatastoreEntryOutput, OperationInput,
    ReadOnlyBytecodeExecution, ReadOnlyCall, SlotAmount,
};
use massa_models::execution::ReadOnlyResult;
use massa_models::operation::OperationDeserializer;
use massa_models::wrapped::WrappedDeserializer;
use massa_models::{
    block::Block, endorsement::WrappedEndorsement, error::ModelsError, operation::WrappedOperation,
    timeslots,
};
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolCommandSender;
use massa_serialization::{DeserializeError, Deserializer};

use itertools::{izip, Itertools};
use massa_models::datastore::DatastoreDeserializer;
use massa_models::{
    address::Address,
    api::{
        AddressInfo, BlockInfo, BlockInfoContent, BlockSummary, EndorsementInfo, EventFilter,
        NodeStatus, OperationInfo, TimeInterval,
    },
    block::BlockId,
    clique::Clique,
    composite::PubkeySig,
    config::CompactConfig,
    endorsement::EndorsementId,
    execution::ExecuteReadOnlyResponse,
    node::NodeId,
    operation::OperationId,
    output_event::SCOutputEvent,
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
    timeslots::{get_latest_block_slot_at_timestamp, time_range_to_slot_range},
    version::Version,
};
use massa_network_exports::{NetworkCommandSender, NetworkConfig};
use massa_pool_exports::PoolController;
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;
use std::net::{IpAddr, SocketAddr};

impl API<Public> {
    /// generate a new public API
    pub fn new(
        consensus_command_sender: ConsensusCommandSender,
        execution_controller: Box<dyn ExecutionController>,
        api_settings: APIConfig,
        selector_controller: Box<dyn SelectorController>,
        consensus_settings: ConsensusConfig,
        pool_command_sender: Box<dyn PoolController>,
        protocol_command_sender: ProtocolCommandSender,
        network_settings: NetworkConfig,
        version: Version,
        network_command_sender: NetworkCommandSender,
        compensation_millis: i64,
        node_id: NodeId,
        storage: Storage,
    ) -> Self {
        API(Public {
            consensus_command_sender,
            consensus_config: consensus_settings,
            api_settings,
            pool_command_sender,
            network_settings,
            version,
            network_command_sender,
            protocol_command_sender,
            compensation_millis,
            node_id,
            execution_controller,
            selector_controller,
            storage,
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

    fn add_staking_secret_keys(&self, _: Vec<String>) -> BoxFuture<Result<(), ApiError>> {
        crate::wrong_api::<()>()
    }

    fn execute_read_only_bytecode(
        &self,
        reqs: Vec<ReadOnlyBytecodeExecution>,
    ) -> BoxFuture<Result<Vec<ExecuteReadOnlyResponse>, ApiError>> {
        if reqs.len() as u64 > self.0.api_settings.max_arguments {
            let closure = async move || Err(ApiError::BadRequest("too many arguments".into()));
            return Box::pin(closure());
        }

        let mut res: Vec<ExecuteReadOnlyResponse> = Vec::with_capacity(reqs.len());
        for ReadOnlyBytecodeExecution {
            max_gas,
            address,
            simulated_gas_price,
            bytecode,
            operation_datastore,
        } in reqs
        {
            let address = address.unwrap_or_else(|| {
                // if no addr provided, use a random one
                Address::from_public_key(&KeyPair::generate().get_public_key())
            });

            let op_datastore = match operation_datastore {
                Some(v) => {
                    let deserializer = DatastoreDeserializer::new(
                        self.0.api_settings.max_op_datastore_entry_count,
                        self.0.api_settings.max_op_datastore_key_length,
                        self.0.api_settings.max_op_datastore_value_length,
                    );
                    match deserializer.deserialize::<DeserializeError>(&v) {
                        Ok((_, deserialized)) => Some(deserialized),
                        Err(e) => {
                            let err_str = format!("Operation datastore error: {}", e);
                            let closure = async move || Err(ApiError::InconsistencyError(err_str));
                            return Box::pin(closure());
                        }
                    }
                }
                None => None,
            };

            // TODO:
            // * set a maximum gas value for read-only executions to prevent attacks
            // * stop mapping request and result, reuse execution's structures
            // * remove async stuff

            // translate request
            let req = ReadOnlyExecutionRequest {
                max_gas,
                simulated_gas_price,
                target: ReadOnlyExecutionTarget::BytecodeExecution(bytecode),
                call_stack: vec![ExecutionStackElement {
                    address,
                    coins: Default::default(),
                    owned_addresses: vec![address],
                    operation_datastore: op_datastore,
                }],
            };

            // run
            let result = self.0.execution_controller.execute_readonly_request(req);

            // map result
            let result = ExecuteReadOnlyResponse {
                executed_at: result
                    .as_ref()
                    .map_or_else(|_| Slot::new(0, 0), |v| v.out.slot),
                result: result.as_ref().map_or_else(
                    |err| ReadOnlyResult::Error(format!("readonly call failed: {}", err)),
                    |_| ReadOnlyResult::Ok,
                ),
                gas_cost: result.as_ref().map_or_else(|_| 0, |v| v.gas_cost),
                output_events: result
                    .map_or_else(|_| Default::default(), |mut v| v.out.events.take()),
            };

            res.push(result);
        }

        // return result
        let closure = async move || Ok(res);
        Box::pin(closure())
    }

    fn execute_read_only_call(
        &self,
        reqs: Vec<ReadOnlyCall>,
    ) -> BoxFuture<Result<Vec<ExecuteReadOnlyResponse>, ApiError>> {
        if reqs.len() as u64 > self.0.api_settings.max_arguments {
            let closure = async move || Err(ApiError::BadRequest("too many arguments".into()));
            return Box::pin(closure());
        }

        let mut res: Vec<ExecuteReadOnlyResponse> = Vec::with_capacity(reqs.len());
        for ReadOnlyCall {
            max_gas,
            simulated_gas_price,
            target_address,
            target_function,
            parameter,
            caller_address,
        } in reqs
        {
            let caller_address = caller_address.unwrap_or_else(|| {
                // if no addr provided, use a random one
                Address::from_public_key(&KeyPair::generate().get_public_key())
            });

            // TODO:
            // * set a maximum gas value for read-only executions to prevent attacks
            // * stop mapping request and result, reuse execution's structures
            // * remove async stuff

            // translate request
            let req = ReadOnlyExecutionRequest {
                max_gas,
                simulated_gas_price,
                target: ReadOnlyExecutionTarget::FunctionCall {
                    target_func: target_function,
                    target_addr: target_address,
                    parameter,
                },
                call_stack: vec![
                    ExecutionStackElement {
                        address: caller_address,
                        coins: Default::default(),
                        owned_addresses: vec![caller_address],
                        operation_datastore: None, // should always be None
                    },
                    ExecutionStackElement {
                        address: target_address,
                        coins: Default::default(),
                        owned_addresses: vec![target_address],
                        operation_datastore: None, // should always be None
                    },
                ],
            };

            // run
            let result = self.0.execution_controller.execute_readonly_request(req);

            // map result
            let result = ExecuteReadOnlyResponse {
                executed_at: result
                    .as_ref()
                    .map_or_else(|_| Slot::new(0, 0), |v| v.out.slot),
                result: result.as_ref().map_or_else(
                    |err| ReadOnlyResult::Error(format!("readonly call failed: {}", err)),
                    |_| ReadOnlyResult::Ok,
                ),
                gas_cost: result.as_ref().map_or_else(|_| 0, |v| v.gas_cost),
                output_events: result
                    .map_or_else(|_| Default::default(), |mut v| v.out.events.take()),
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

    fn get_staking_addresses(&self) -> BoxFuture<Result<PreHashSet<Address>, ApiError>> {
        crate::wrong_api::<PreHashSet<Address>>()
    }

    fn node_ban_by_ip(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        crate::wrong_api::<()>()
    }

    fn node_ban_by_id(&self, _: Vec<NodeId>) -> BoxFuture<Result<(), ApiError>> {
        crate::wrong_api::<()>()
    }

    fn node_unban_by_ip(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        crate::wrong_api::<()>()
    }

    fn node_unban_by_id(&self, _: Vec<NodeId>) -> BoxFuture<Result<(), ApiError>> {
        crate::wrong_api::<()>()
    }

    fn get_status(&self) -> BoxFuture<Result<NodeStatus, ApiError>> {
        let execution_controller = self.0.execution_controller.clone();
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let network_command_sender = self.0.network_command_sender.clone();
        let network_config = self.0.network_settings.clone();
        let version = self.0.version;
        let consensus_settings = self.0.consensus_config.clone();
        let compensation_millis = self.0.compensation_millis;
        let pool_command_sender = self.0.pool_command_sender.clone();
        let node_id = self.0.node_id;
        let config = CompactConfig::default();
        let closure = async move || {
            let now = MassaTime::now(compensation_millis)?;
            let last_slot = get_latest_block_slot_at_timestamp(
                consensus_settings.thread_count,
                consensus_settings.t0,
                consensus_settings.genesis_timestamp,
                now,
            )?;

            let execution_stats = execution_controller.get_stats();

            let (consensus_stats, network_stats, peers) = tokio::join!(
                consensus_command_sender.get_stats(),
                network_command_sender.get_network_stats(),
                network_command_sender.get_peers()
            );

            let pool_stats = (
                pool_command_sender.get_operation_count(),
                pool_command_sender.get_endorsement_count(),
            );

            Ok(NodeStatus {
                node_id,
                node_ip: network_config.routable_ip,
                version,
                current_time: now,
                connected_nodes: peers?
                    .peers
                    .iter()
                    .flat_map(|(ip, peer)| {
                        peer.active_nodes
                            .iter()
                            .map(move |(id, is_outgoing)| (*id, (*ip, *is_outgoing)))
                    })
                    .collect(),
                last_slot,
                next_slot: last_slot
                    .unwrap_or_else(|| Slot::new(0, 0))
                    .get_next_slot(consensus_settings.thread_count)?,
                execution_stats,
                consensus_stats: consensus_stats?,
                network_stats: network_stats?,
                pool_stats,
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

    fn get_stakers(&self) -> BoxFuture<Result<Vec<(Address, u64)>, ApiError>> {
        let execution_controller = self.0.execution_controller.clone();
        let cfg = self.0.consensus_config.clone();
        let compensation_millis = self.0.compensation_millis;

        let closure = async move || {
            let curr_cycle = get_latest_block_slot_at_timestamp(
                cfg.thread_count,
                cfg.t0,
                cfg.genesis_timestamp,
                MassaTime::now(compensation_millis)?,
            )?
            .unwrap_or_else(|| Slot::new(0, 0))
            .get_cycle(cfg.periods_per_cycle);
            let mut staker_vec = execution_controller
                .get_cycle_active_rolls(curr_cycle)
                .into_iter()
                .collect::<Vec<(Address, u64)>>();
            staker_vec.sort_by(|&(_, roll_counts_a), &(_, roll_counts_b)| {
                roll_counts_b.cmp(&roll_counts_a)
            });
            Ok(staker_vec)
        };
        Box::pin(closure())
    }

    fn get_operations(
        &self,
        ops: Vec<OperationId>,
    ) -> BoxFuture<Result<Vec<OperationInfo>, ApiError>> {
        // get the operations and the list of blocks that contain them from storage
        let storage_info: Vec<(WrappedOperation, PreHashSet<BlockId>)> = {
            let read_blocks = self.0.storage.read_blocks();
            let read_ops = self.0.storage.read_operations();
            ops.iter()
                .filter_map(|id| {
                    read_ops.get(id).cloned().map(|op| {
                        (
                            op,
                            read_blocks
                                .get_blocks_by_operation(id)
                                .cloned()
                                .unwrap_or_default(),
                        )
                    })
                })
                .collect()
        };

        // keep only the ops found in storage
        let ops: Vec<OperationId> = storage_info.iter().map(|(op, _)| op.id).collect();

        // ask pool whether it carries the operations
        let in_pool = self.0.pool_command_sender.contains_operations(&ops);

        let api_cfg = self.0.api_settings;
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let closure = async move || {
            if ops.len() as u64 > api_cfg.max_arguments {
                return Err(ApiError::BadRequest("too many arguments".into()));
            }

            // check finality by cross-referencing Consensus and looking for final blocks that contain the op
            let is_final: Vec<bool> = {
                let involved_blocks: Vec<BlockId> = storage_info
                    .iter()
                    .flat_map(|(_op, bs)| bs.iter())
                    .unique()
                    .cloned()
                    .collect();
                let involved_block_statuses = consensus_command_sender
                    .get_block_statuses(&involved_blocks)
                    .await?;
                let block_statuses: PreHashMap<BlockId, BlockGraphStatus> = involved_blocks
                    .into_iter()
                    .zip(involved_block_statuses.into_iter())
                    .collect();
                storage_info
                    .iter()
                    .map(|(_op, bs)| {
                        bs.iter()
                            .any(|b| block_statuses.get(b) == Some(&BlockGraphStatus::Final))
                    })
                    .collect()
            };

            // gather all values into a vector of OperationInfo instances
            let mut res: Vec<OperationInfo> = Vec::with_capacity(ops.len());
            let zipped_iterator = izip!(
                ops.into_iter(),
                storage_info.into_iter(),
                in_pool.into_iter(),
                is_final.into_iter()
            );
            for (id, (operation, in_blocks), in_pool, is_final) in zipped_iterator {
                res.push(OperationInfo {
                    id,
                    operation,
                    in_pool,
                    is_final,
                    in_blocks: in_blocks.into_iter().collect(),
                });
            }

            // return values in the right order
            Ok(res)
        };
        Box::pin(closure())
    }

    fn get_endorsements(
        &self,
        eds: Vec<EndorsementId>,
    ) -> BoxFuture<Result<Vec<EndorsementInfo>, ApiError>> {
        // get the endorsements and the list of blocks that contain them from storage
        let storage_info: Vec<(WrappedEndorsement, PreHashSet<BlockId>)> = {
            let read_blocks = self.0.storage.read_blocks();
            let read_endos = self.0.storage.read_endorsements();
            eds.iter()
                .filter_map(|id| {
                    read_endos.get(id).cloned().map(|ed| {
                        (
                            ed,
                            read_blocks
                                .get_blocks_by_endorsement(id)
                                .cloned()
                                .unwrap_or_default(),
                        )
                    })
                })
                .collect()
        };

        // keep only the ops found in storage
        let eds: Vec<EndorsementId> = storage_info.iter().map(|(ed, _)| ed.id).collect();

        // ask pool whether it carries the operations
        let in_pool = self.0.pool_command_sender.contains_endorsements(&eds);

        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let api_cfg = self.0.api_settings;
        let closure = async move || {
            if eds.len() as u64 > api_cfg.max_arguments {
                return Err(ApiError::BadRequest("too many arguments".into()));
            }

            // check finality by cross-referencing Consensus and looking for final blocks that contain the endorsement
            let is_final: Vec<bool> = {
                let involved_blocks: Vec<BlockId> = storage_info
                    .iter()
                    .flat_map(|(_ed, bs)| bs.iter())
                    .unique()
                    .cloned()
                    .collect();
                let involved_block_statuses = consensus_command_sender
                    .get_block_statuses(&involved_blocks)
                    .await?;
                let block_statuses: PreHashMap<BlockId, BlockGraphStatus> = involved_blocks
                    .into_iter()
                    .zip(involved_block_statuses.into_iter())
                    .collect();
                storage_info
                    .iter()
                    .map(|(_ed, bs)| {
                        bs.iter()
                            .any(|b| block_statuses.get(b) == Some(&BlockGraphStatus::Final))
                    })
                    .collect()
            };

            // gather all values into a vector of EndorsementInfo instances
            let mut res: Vec<EndorsementInfo> = Vec::with_capacity(eds.len());
            let zipped_iterator = izip!(
                eds.into_iter(),
                storage_info.into_iter(),
                in_pool.into_iter(),
                is_final.into_iter()
            );
            for (id, (endorsement, in_blocks), in_pool, is_final) in zipped_iterator {
                res.push(EndorsementInfo {
                    id,
                    endorsement,
                    in_pool,
                    is_final,
                    in_blocks: in_blocks.into_iter().collect(),
                });
            }

            // return values in the right order
            Ok(res)
        };
        Box::pin(closure())
    }

    /// gets a block. Returns None if not found
    /// only active blocks are returned
    fn get_block(&self, id: BlockId) -> BoxFuture<Result<BlockInfo, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let storage = self.0.storage.clone_without_refs();
        let closure = async move || {
            let block = match storage.read_blocks().get(&id).cloned() {
                Some(b) => b.content,
                None => {
                    return Ok(BlockInfo { id, content: None });
                }
            };

            let graph_status = consensus_command_sender
                .get_block_statuses(&[id])
                .await?
                .into_iter()
                .next()
                .expect("expected get_block_statuses to return one element");

            let is_final = graph_status == BlockGraphStatus::Final;
            let is_in_blockclique = graph_status == BlockGraphStatus::ActiveInBlockclique;
            let is_candidate = graph_status == BlockGraphStatus::ActiveInBlockclique
                || graph_status == BlockGraphStatus::ActiveInAlternativeCliques;
            let is_discarded = graph_status == BlockGraphStatus::Discarded;

            Ok(BlockInfo {
                id,
                content: Some(BlockInfoContent {
                    is_final,
                    is_in_blockclique,
                    is_candidate,
                    is_discarded,
                    block,
                }),
            })
        };
        Box::pin(closure())
    }

    fn get_blockclique_block_by_slot(
        &self,
        slot: Slot,
    ) -> BoxFuture<Result<Option<Block>, ApiError>> {
        let consensus_command_sender = self.0.consensus_command_sender.clone();
        let storage = self.0.storage.clone_without_refs();
        let closure = async move || {
            let block_id_result = consensus_command_sender
                .get_blockclique_block_at_slot(slot)
                .await;
            let block_id = match block_id_result? {
                Some(id) => id,
                None => return Ok(None),
            };
            let res = storage
                .read_blocks()
                .get(&block_id)
                .map(|b| b.content.clone());
            Ok(res)
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
                    creator: exported_block.header.creator_address,
                    parents: exported_block.header.content.parents,
                });
            }
            for (id, (reason, (slot, creator, parents))) in graph.discarded_blocks.into_iter() {
                if reason == DiscardReason::Stale {
                    res.push(BlockSummary {
                        id,
                        is_final: false,
                        is_stale: true,
                        is_in_blockclique: false,
                        slot,
                        creator,
                        parents,
                    });
                }
            }
            Ok(res)
        };
        Box::pin(closure())
    }

    fn get_datastore_entries(
        &self,
        entries: Vec<DatastoreEntryInput>,
    ) -> BoxFuture<Result<Vec<DatastoreEntryOutput>, ApiError>> {
        let execution_controller = self.0.execution_controller.clone();
        let closure = async move || {
            Ok(execution_controller
                .get_final_and_active_data_entry(
                    entries
                        .into_iter()
                        .map(|input| (input.address, input.key))
                        .collect::<Vec<_>>(),
                )
                .into_iter()
                .map(|output| DatastoreEntryOutput {
                    final_value: output.0,
                    candidate_value: output.1,
                })
                .collect())
        };
        Box::pin(closure())
    }

    fn get_addresses(
        &self,
        addresses: Vec<Address>,
    ) -> BoxFuture<Result<Vec<AddressInfo>, ApiError>> {
        // get info from storage about which blocks the addresses have created
        let created_blocks: Vec<PreHashSet<BlockId>> = {
            let lck = self.0.storage.read_blocks();
            addresses
                .iter()
                .map(|address| {
                    lck.get_blocks_created_by(address)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect()
        };

        // get info from storage about which operations the addresses have created
        let created_operations: Vec<PreHashSet<OperationId>> = {
            let lck = self.0.storage.read_operations();
            addresses
                .iter()
                .map(|address| {
                    lck.get_operations_created_by(address)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect()
        };

        // get info from storage about which endorsements the addresses have created
        let created_endorsements: Vec<PreHashSet<EndorsementId>> = {
            let lck = self.0.storage.read_endorsements();
            addresses
                .iter()
                .map(|address| {
                    lck.get_endorsements_created_by(address)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect()
        };

        // get execution info
        let execution_infos = self.0.execution_controller.get_addresses_infos(&addresses);

        // get future draws from selector
        let selection_draws = {
            let cur_slot = timeslots::get_current_latest_block_slot(
                self.0.consensus_config.thread_count,
                self.0.consensus_config.t0,
                self.0.consensus_config.genesis_timestamp,
                self.0.compensation_millis,
            )
            .expect("could not get latest current slot")
            .unwrap_or_else(|| Slot::new(0, 0));
            let slot_end = Slot::new(
                cur_slot
                    .period
                    .saturating_add(self.0.api_settings.draw_lookahead_period_count),
                cur_slot.thread,
            );
            addresses
                .iter()
                .map(|addr| {
                    self.0
                        .selector_controller
                        .get_address_selections(addr, cur_slot, slot_end)
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
        };

        // compile results
        let mut res = Vec::with_capacity(addresses.len());
        let iterator = izip!(
            addresses.into_iter(),
            created_blocks.into_iter(),
            created_operations.into_iter(),
            created_endorsements.into_iter(),
            execution_infos.into_iter(),
            selection_draws.into_iter(),
        );
        for (
            address,
            created_blocks,
            created_operations,
            created_endorsements,
            execution_infos,
            (next_block_draws, next_endorsement_draws),
        ) in iterator
        {
            res.push(AddressInfo {
                // general address info
                address,
                thread: address.get_thread(self.0.consensus_config.thread_count),

                // final execution info
                final_balance: execution_infos.final_balance,
                final_roll_count: execution_infos.final_roll_count,
                final_datastore_keys: execution_infos
                    .final_datastore_keys
                    .into_iter()
                    .collect::<Vec<_>>(),

                // candidate execution info
                candidate_balance: execution_infos.candidate_balance,
                candidate_roll_count: execution_infos.candidate_roll_count,
                candidate_datastore_keys: execution_infos
                    .candidate_datastore_keys
                    .into_iter()
                    .collect::<Vec<_>>(),

                // deferred credits
                deferred_credits: execution_infos
                    .future_deferred_credits
                    .into_iter()
                    .map(|(slot, amount)| SlotAmount { slot, amount })
                    .collect::<Vec<_>>(),

                // selector info
                next_block_draws,
                next_endorsement_draws,

                // created objects
                created_blocks: created_blocks.into_iter().collect::<Vec<_>>(),
                created_endorsements: created_endorsements.into_iter().collect::<Vec<_>>(),
                created_operations: created_operations.into_iter().collect::<Vec<_>>(),

                // cycle infos
                cycle_infos: execution_infos.cycle_infos,
            });
        }

        let closure = async move || Ok(res);
        Box::pin(closure())
    }

    fn send_operations(
        &self,
        ops: Vec<OperationInput>,
    ) -> BoxFuture<Result<Vec<OperationId>, ApiError>> {
        let mut cmd_sender = self.0.pool_command_sender.clone();
        let mut protocol_sender = self.0.protocol_command_sender.clone();
        let api_cfg = self.0.api_settings;
        let mut to_send = self.0.storage.clone_without_refs();
        let closure = async move || {
            if ops.len() as u64 > api_cfg.max_arguments {
                return Err(ApiError::BadRequest("too many arguments".into()));
            }
            let operation_deserializer = WrappedDeserializer::new(OperationDeserializer::new(
                api_cfg.max_datastore_value_length,
                api_cfg.max_function_name_length,
                api_cfg.max_parameter_size,
                api_cfg.max_op_datastore_entry_count,
                api_cfg.max_op_datastore_key_length,
                api_cfg.max_op_datastore_value_length,
            ));
            let verified_ops = ops
                .into_iter()
                .map(|op_input| {
                    let mut op_serialized = Vec::new();
                    op_serialized.extend(op_input.signature.to_bytes());
                    op_serialized.extend(op_input.creator_public_key.to_bytes());
                    op_serialized.extend(op_input.serialized_content);
                    let (rest, op): (&[u8], WrappedOperation) = operation_deserializer
                        .deserialize::<DeserializeError>(&op_serialized)
                        .map_err(|err| {
                            ApiError::ModelsError(ModelsError::DeserializeError(err.to_string()))
                        })?;
                    if rest.is_empty() {
                        Ok(op)
                    } else {
                        Err(ApiError::ModelsError(ModelsError::DeserializeError(
                            "There is data left after operation deserialization".to_owned(),
                        )))
                    }
                })
                .map(|op| match op {
                    Ok(operation) => {
                        operation.verify_signature()?;
                        Ok(operation)
                    }
                    Err(e) => Err(e),
                })
                .collect::<Result<Vec<WrappedOperation>, ApiError>>()?;
            to_send.store_operations(verified_ops.clone());
            let ids: Vec<OperationId> = verified_ops.iter().map(|op| op.id).collect();
            cmd_sender.add_operations(to_send.clone());
            protocol_sender.propagate_operations(to_send).await?;
            Ok(ids)
        };
        Box::pin(closure())
    }

    /// Get events optionally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    fn get_filtered_sc_output_event(
        &self,
        filter: EventFilter,
    ) -> BoxFuture<Result<Vec<SCOutputEvent>, ApiError>> {
        let events = self
            .0
            .execution_controller
            .get_filtered_sc_output_event(filter);

        // TODO: get rid of the async part
        let closure = async move || Ok(events);
        Box::pin(closure())
    }

    fn node_whitelist(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        crate::wrong_api::<()>()
    }

    fn node_remove_from_whitelist(&self, _: Vec<IpAddr>) -> BoxFuture<Result<(), ApiError>> {
        crate::wrong_api::<()>()
    }
}
