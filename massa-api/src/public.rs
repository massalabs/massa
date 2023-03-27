//! Copyright (c) 2022 MASSA LABS <info@massa.net>
#![allow(clippy::too_many_arguments)]

use crate::{MassaRpcServer, Public, RpcServer, StopHandle, Value, API};
use async_trait::async_trait;
use jsonrpsee::core::{Error as JsonRpseeError, RpcResult};
use massa_api_exports::{
    address::AddressInfo,
    block::{BlockInfo, BlockInfoContent, BlockSummary},
    config::APIConfig,
    datastore::{DatastoreEntryInput, DatastoreEntryOutput},
    endorsement::EndorsementInfo,
    error::ApiError,
    execution::{ExecuteReadOnlyResponse, ReadOnlyBytecodeExecution, ReadOnlyCall, ReadOnlyResult},
    node::NodeStatus,
    operation::{OperationInfo, OperationInput},
    page::{PageRequest, PagedVec},
    slot::SlotAmount,
    TimeInterval,
};
use massa_consensus_exports::block_status::DiscardReason;
use massa_consensus_exports::ConsensusController;
use massa_execution_exports::{
    ExecutionController, ExecutionStackElement, ReadOnlyExecutionRequest, ReadOnlyExecutionTarget,
};
use massa_models::operation::OperationDeserializer;
use massa_models::secure_share::SecureShareDeserializer;
use massa_models::{
    block::{Block, BlockGraphStatus},
    endorsement::SecureShareEndorsement,
    error::ModelsError,
    operation::SecureShareOperation,
    timeslots,
};
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolCommandSender;
use massa_serialization::{DeserializeError, Deserializer};

use itertools::{izip, Itertools};
use massa_models::datastore::DatastoreDeserializer;
use massa_models::{
    address::Address,
    block_id::BlockId,
    clique::Clique,
    composite::PubkeySig,
    config::CompactConfig,
    endorsement::EndorsementId,
    execution::EventFilter,
    node::NodeId,
    operation::OperationId,
    output_event::SCOutputEvent,
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
    timeslots::{get_latest_block_slot_at_timestamp, time_range_to_slot_range},
    version::Version,
};
use massa_network_exports::{NetworkCommandSender, NetworkCommandSenderTrait, NetworkConfig};
use massa_pool_exports::PoolController;
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};

impl API<Public> {
    /// generate a new public API
    pub fn new(
        consensus_controller: Box<dyn ConsensusController>,
        execution_controller: Box<dyn ExecutionController>,
        api_settings: APIConfig,
        selector_controller: Box<dyn SelectorController>,
        pool_command_sender: Box<dyn PoolController>,
        protocol_command_sender: ProtocolCommandSender,
        network_settings: NetworkConfig,
        version: Version,
        network_command_sender: NetworkCommandSender,
        node_id: NodeId,
        storage: Storage,
    ) -> Self {
        API(Public {
            consensus_controller,
            api_settings,
            pool_command_sender,
            network_settings,
            version,
            network_command_sender,
            protocol_command_sender,
            node_id,
            execution_controller,
            selector_controller,
            storage,
        })
    }
}

#[async_trait]
impl RpcServer for API<Public> {
    async fn serve(
        self,
        url: &SocketAddr,
        api_config: &APIConfig,
    ) -> Result<StopHandle, JsonRpseeError> {
        crate::serve(self.into_rpc(), url, api_config).await
    }
}

#[doc(hidden)]
#[async_trait]
impl MassaRpcServer for API<Public> {
    async fn stop_node(&self) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn node_sign_message(&self, _: Vec<u8>) -> RpcResult<PubkeySig> {
        crate::wrong_api::<PubkeySig>()
    }

    async fn add_staking_secret_keys(&self, _: Vec<String>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn execute_read_only_bytecode(
        &self,
        reqs: Vec<ReadOnlyBytecodeExecution>,
    ) -> RpcResult<Vec<ExecuteReadOnlyResponse>> {
        if reqs.len() as u64 > self.0.api_settings.max_arguments {
            return Err(ApiError::BadRequest("too many arguments".into()).into());
        }

        let mut res: Vec<ExecuteReadOnlyResponse> = Vec::with_capacity(reqs.len());
        for ReadOnlyBytecodeExecution {
            max_gas,
            address,
            bytecode,
            operation_datastore,
            is_final,
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
                            return Err(ApiError::InconsistencyError(format!(
                                "Operation datastore error: {}",
                                e
                            ))
                            .into())
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
                target: ReadOnlyExecutionTarget::BytecodeExecution(bytecode),
                call_stack: vec![ExecutionStackElement {
                    address,
                    coins: Default::default(),
                    owned_addresses: vec![address],
                    operation_datastore: op_datastore,
                }],
                is_final,
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
                    |res| ReadOnlyResult::Ok(res.call_result.clone()),
                ),
                gas_cost: result.as_ref().map_or_else(|_| 0, |v| v.gas_cost),
                output_events: result
                    .as_ref()
                    .map_or_else(|_| Default::default(), |v| v.out.events.clone().0),
                state_changes: result.map_or_else(|_| Default::default(), |v| v.out.state_changes),
            };

            res.push(result);
        }

        // return result
        Ok(res)
    }

    async fn execute_read_only_call(
        &self,
        reqs: Vec<ReadOnlyCall>,
    ) -> RpcResult<Vec<ExecuteReadOnlyResponse>> {
        if reqs.len() as u64 > self.0.api_settings.max_arguments {
            return Err(ApiError::BadRequest("too many arguments".into()).into());
        }

        let mut res: Vec<ExecuteReadOnlyResponse> = Vec::with_capacity(reqs.len());
        for ReadOnlyCall {
            max_gas,
            target_address,
            target_function,
            parameter,
            caller_address,
            is_final,
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
                is_final,
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
                    |res| ReadOnlyResult::Ok(res.call_result.clone()),
                ),
                gas_cost: result.as_ref().map_or_else(|_| 0, |v| v.gas_cost),
                output_events: result
                    .as_ref()
                    .map_or_else(|_| Default::default(), |v| v.out.events.clone().0),
                state_changes: result.map_or_else(|_| Default::default(), |v| v.out.state_changes),
            };

            res.push(result);
        }

        // return result
        Ok(res)
    }

    async fn remove_staking_addresses(&self, _: Vec<Address>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn get_staking_addresses(&self) -> RpcResult<PreHashSet<Address>> {
        crate::wrong_api::<PreHashSet<Address>>()
    }

    async fn node_ban_by_ip(&self, _: Vec<IpAddr>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn node_ban_by_id(&self, _: Vec<NodeId>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn node_unban_by_ip(&self, _: Vec<IpAddr>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn node_unban_by_id(&self, _: Vec<NodeId>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn get_status(&self) -> RpcResult<NodeStatus> {
        let execution_controller = self.0.execution_controller.clone();
        let consensus_controller = self.0.consensus_controller.clone();
        let network_command_sender = self.0.network_command_sender.clone();
        let network_config = self.0.network_settings.clone();
        let version = self.0.version;
        let api_settings = self.0.api_settings.clone();
        let pool_command_sender = self.0.pool_command_sender.clone();
        let node_id = self.0.node_id;
        let config = CompactConfig::default();
        let now = match MassaTime::now() {
            Ok(now) => now,
            Err(e) => return Err(ApiError::TimeError(e).into()),
        };

        let last_slot_result = get_latest_block_slot_at_timestamp(
            api_settings.thread_count,
            api_settings.t0,
            api_settings.genesis_timestamp,
            now,
        );
        let last_slot = match last_slot_result {
            Ok(last_slot) => last_slot,
            Err(e) => return Err(ApiError::ModelsError(e).into()),
        };

        let execution_stats = execution_controller.get_stats();
        let consensus_stats_result = consensus_controller.get_stats();
        let consensus_stats = match consensus_stats_result {
            Ok(consensus_stats) => consensus_stats,
            Err(e) => return Err(ApiError::ConsensusError(e).into()),
        };

        let (network_stats_result, peers_result) = tokio::join!(
            network_command_sender.get_network_stats(),
            network_command_sender.get_peers()
        );

        let network_stats = match network_stats_result {
            Ok(network_stats) => network_stats,
            Err(e) => return Err(ApiError::NetworkError(e).into()),
        };

        let peers = match peers_result {
            Ok(peers) => peers,
            Err(e) => return Err(ApiError::NetworkError(e).into()),
        };

        let pool_stats = (
            pool_command_sender.get_operation_count(),
            pool_command_sender.get_endorsement_count(),
        );

        let next_slot_result = last_slot
            .unwrap_or_else(|| Slot::new(0, 0))
            .get_next_slot(api_settings.thread_count);

        let next_slot = match next_slot_result {
            Ok(next_slot) => next_slot,
            Err(e) => return Err(ApiError::ModelsError(e).into()),
        };

        let connected_nodes = peers
            .peers
            .iter()
            .flat_map(|(ip, peer)| {
                peer.active_nodes
                    .iter()
                    .map(move |(id, is_outgoing)| (*id, (*ip, *is_outgoing)))
            })
            .collect::<BTreeMap<_, _>>();

        let current_cycle = last_slot
            .unwrap_or_else(|| Slot::new(0, 0))
            .get_cycle(api_settings.periods_per_cycle);

        let cycle_duration = match api_settings.t0.checked_mul(api_settings.periods_per_cycle) {
            Ok(cycle_duration) => cycle_duration,
            Err(e) => return Err(ApiError::TimeError(e).into()),
        };

        let current_cycle_time_result = if current_cycle == 0 {
            Ok(api_settings.genesis_timestamp)
        } else {
            cycle_duration.checked_mul(current_cycle).and_then(
                |elapsed_time_before_current_cycle| {
                    api_settings
                        .genesis_timestamp
                        .checked_add(elapsed_time_before_current_cycle)
                },
            )
        };

        let current_cycle_time = match current_cycle_time_result {
            Ok(current_cycle_time) => current_cycle_time,
            Err(e) => return Err(ApiError::TimeError(e).into()),
        };

        let next_cycle_time = match current_cycle_time.checked_add(cycle_duration) {
            Ok(next_cycle_time) => next_cycle_time,
            Err(e) => return Err(ApiError::TimeError(e).into()),
        };

        Ok(NodeStatus {
            node_id,
            node_ip: network_config.routable_ip,
            version,
            current_time: now,
            current_cycle_time,
            next_cycle_time,
            connected_nodes,
            last_slot,
            next_slot,
            execution_stats,
            consensus_stats,
            network_stats,
            pool_stats,
            config,
            current_cycle,
        })
    }

    async fn get_cliques(&self) -> RpcResult<Vec<Clique>> {
        let consensus_controller = self.0.consensus_controller.clone();
        Ok(consensus_controller.get_cliques())
    }

    async fn get_stakers(
        &self,
        page_request: Option<PageRequest>,
    ) -> RpcResult<PagedVec<(Address, u64)>> {
        let execution_controller = self.0.execution_controller.clone();
        let cfg = self.0.api_settings.clone();

        let now = match MassaTime::now() {
            Ok(now) => now,
            Err(e) => return Err(ApiError::TimeError(e).into()),
        };

        let latest_block_slot_at_timestamp_result = get_latest_block_slot_at_timestamp(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            now,
        );

        let curr_cycle = match latest_block_slot_at_timestamp_result {
            Ok(curr_cycle) => curr_cycle
                .unwrap_or_else(|| Slot::new(0, 0))
                .get_cycle(cfg.periods_per_cycle),
            Err(e) => return Err(ApiError::ModelsError(e).into()),
        };

        let mut staker_vec = execution_controller
            .get_cycle_active_rolls(curr_cycle)
            .into_iter()
            .collect::<Vec<(Address, u64)>>();

        staker_vec
            .sort_by(|&(_, roll_counts_a), &(_, roll_counts_b)| roll_counts_b.cmp(&roll_counts_a));

        let paged_vec = PagedVec::new(staker_vec, page_request);

        Ok(paged_vec)
    }

    async fn get_operations(&self, ops: Vec<OperationId>) -> RpcResult<Vec<OperationInfo>> {
        // get the operations and the list of blocks that contain them from storage
        let storage_info: Vec<(SecureShareOperation, PreHashSet<BlockId>)> = {
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

        // keep only the ops id (found in storage)
        let ops: Vec<OperationId> = storage_info.iter().map(|(op, _)| op.id).collect();

        let api_cfg = self.0.api_settings.clone();
        if ops.len() as u64 > api_cfg.max_arguments {
            return Err(ApiError::BadRequest("too many arguments".into()).into());
        }

        // ask pool whether it carries the operations
        let in_pool = self.0.pool_command_sender.contains_operations(&ops);

        let (speculative_op_exec_statuses, final_op_exec_statuses) =
            self.0.execution_controller.get_op_exec_status();

        // compute operation finality and operation execution status from *_op_exec_statuses
        let (is_operation_final, statuses): (Vec<Option<bool>>, Vec<Option<bool>>) = ops
            .iter()
            .map(|op| {
                match (
                    final_op_exec_statuses.get(op),
                    speculative_op_exec_statuses.get(op),
                ) {
                    // op status found in the final hashmap, so the op is "final"(first value of the tuple: Some(true))
                    // and we keep its status (copied as the second value of the tuple)
                    (Some(val), _) => (Some(true), Some(*val)),
                    // op status NOT found in the final hashmap but in the speculative one, so the op is "not final"(first value of the tuple: Some(false))
                    // and we keep its status (copied as the second value of the tuple)
                    (None, Some(val)) => (Some(false), Some(*val)),
                    // op status not defined in any hashmap, finality and status are unknow hence (None, None)
                    (None, None) => (None, None),
                }
            })
            .collect::<Vec<(Option<bool>, Option<bool>)>>()
            .into_iter()
            .unzip();

        // gather all values into a vector of OperationInfo instances
        let mut res: Vec<OperationInfo> = Vec::with_capacity(ops.len());
        let zipped_iterator = izip!(
            ops.into_iter(),
            storage_info.into_iter(),
            in_pool.into_iter(),
            is_operation_final.into_iter(),
            statuses.into_iter(),
        );
        for (id, (operation, in_blocks), in_pool, is_operation_final, op_exec_status) in
            zipped_iterator
        {
            res.push(OperationInfo {
                id,
                in_pool,
                is_operation_final,
                thread: operation
                    .content_creator_address
                    .get_thread(api_cfg.thread_count),
                operation,
                in_blocks: in_blocks.into_iter().collect(),
                op_exec_status,
            });
        }

        // return values in the right order
        Ok(res)
    }

    async fn get_endorsements(&self, eds: Vec<EndorsementId>) -> RpcResult<Vec<EndorsementInfo>> {
        // get the endorsements and the list of blocks that contain them from storage
        let storage_info: Vec<(SecureShareEndorsement, PreHashSet<BlockId>)> = {
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

        let consensus_controller = self.0.consensus_controller.clone();
        let api_cfg = self.0.api_settings.clone();

        if eds.len() as u64 > api_cfg.max_arguments {
            return Err(ApiError::BadRequest("too many arguments".into()).into());
        }

        // check finality by cross-referencing Consensus and looking for final blocks that contain the endorsement
        let is_final: Vec<bool> = {
            let involved_blocks: Vec<BlockId> = storage_info
                .iter()
                .flat_map(|(_ed, bs)| bs.iter())
                .unique()
                .cloned()
                .collect();

            let involved_block_statuses = consensus_controller.get_block_statuses(&involved_blocks);

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
    }

    /// gets a block(s). Returns nothing if not found
    /// only active blocks are returned
    async fn get_blocks(&self, ids: Vec<BlockId>) -> RpcResult<Vec<BlockInfo>> {
        let consensus_controller = self.0.consensus_controller.clone();
        let storage = self.0.storage.clone_without_refs();
        let blocks = ids
            .into_iter()
            .filter_map(|id| {
                let content = if let Some(wrapped_block) = storage.read_blocks().get(&id) {
                    wrapped_block.content.clone()
                } else {
                    return None;
                };

                if let Some(graph_status) = consensus_controller
                    .get_block_statuses(&[id])
                    .into_iter()
                    .next()
                {
                    let is_final = graph_status == BlockGraphStatus::Final;
                    let is_in_blockclique = graph_status == BlockGraphStatus::ActiveInBlockclique;
                    let is_candidate = graph_status == BlockGraphStatus::ActiveInBlockclique
                        || graph_status == BlockGraphStatus::ActiveInAlternativeCliques;
                    let is_discarded = graph_status == BlockGraphStatus::Discarded;

                    return Some(BlockInfo {
                        id,
                        content: Some(BlockInfoContent {
                            is_final,
                            is_in_blockclique,
                            is_candidate,
                            is_discarded,
                            block: content,
                        }),
                    });
                }

                None
            })
            .collect::<Vec<BlockInfo>>();

        Ok(blocks)
    }

    async fn get_blockclique_block_by_slot(&self, slot: Slot) -> RpcResult<Option<Block>> {
        let consensus_controller = self.0.consensus_controller.clone();
        let storage = self.0.storage.clone_without_refs();

        let block_id_option = consensus_controller.get_blockclique_block_at_slot(slot);

        let block_id = match block_id_option {
            Some(id) => id,
            None => return Ok(None),
        };

        let res = storage
            .read_blocks()
            .get(&block_id)
            .map(|b| b.content.clone());
        Ok(res)
    }

    /// gets an interval of the block graph from consensus, with time filtering
    /// time filtering is done consensus-side to prevent communication overhead
    async fn get_graph_interval(&self, time: TimeInterval) -> RpcResult<Vec<BlockSummary>> {
        let consensus_controller = self.0.consensus_controller.clone();
        let api_settings = self.0.api_settings.clone();

        // filter blocks from graph_export
        let time_range_to_slot_range_result = time_range_to_slot_range(
            api_settings.thread_count,
            api_settings.t0,
            api_settings.genesis_timestamp,
            time.start,
            time.end,
        );

        let (start_slot, end_slot) = match time_range_to_slot_range_result {
            Ok(time_range_to_slot_range) => time_range_to_slot_range,
            Err(e) => return Err(ApiError::ModelsError(e).into()),
        };

        let graph = match consensus_controller.get_block_graph_status(start_slot, end_slot) {
            Ok(graph) => graph,
            Err(e) => return Err(ApiError::ConsensusError(e).into()),
        };

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
                creator: exported_block.header.content_creator_address,
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
    }

    async fn get_datastore_entries(
        &self,
        entries: Vec<DatastoreEntryInput>,
    ) -> RpcResult<Vec<DatastoreEntryOutput>> {
        let execution_controller = self.0.execution_controller.clone();
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
    }

    async fn get_addresses(&self, addresses: Vec<Address>) -> RpcResult<Vec<AddressInfo>> {
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
                self.0.api_settings.thread_count,
                self.0.api_settings.t0,
                self.0.api_settings.genesis_timestamp,
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
                thread: address.get_thread(self.0.api_settings.thread_count),

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

        Ok(res)
    }

    async fn send_operations(&self, ops: Vec<OperationInput>) -> RpcResult<Vec<OperationId>> {
        let mut cmd_sender = self.0.pool_command_sender.clone();
        let mut protocol_sender = self.0.protocol_command_sender.clone();
        let api_cfg = self.0.api_settings.clone();
        let mut to_send = self.0.storage.clone_without_refs();

        if ops.len() as u64 > api_cfg.max_arguments {
            return Err(ApiError::BadRequest("too many arguments".into()).into());
        }
        let operation_deserializer = SecureShareDeserializer::new(OperationDeserializer::new(
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
                let (rest, op): (&[u8], SecureShareOperation) = operation_deserializer
                    .deserialize::<DeserializeError>(&op_serialized)
                    .map_err(|err| {
                        ApiError::ModelsError(ModelsError::DeserializeError(err.to_string()))
                    })?;
                if rest.is_empty() {
                    Ok(op)
                } else {
                    Err(ApiError::ModelsError(ModelsError::DeserializeError(
                        "There is data left after operation deserialization".to_owned(),
                    ))
                    .into())
                }
            })
            .map(|op| match op {
                Ok(operation) => {
                    let _verify_signature = match operation.verify_signature() {
                        Ok(()) => (),
                        Err(e) => return Err(ApiError::ModelsError(e).into()),
                    };
                    Ok(operation)
                }
                Err(e) => Err(e),
            })
            .collect::<RpcResult<Vec<SecureShareOperation>>>()?;
        to_send.store_operations(verified_ops.clone());
        let ids: Vec<OperationId> = verified_ops.iter().map(|op| op.id).collect();
        cmd_sender.add_operations(to_send.clone());

        tokio::task::spawn_blocking(move || protocol_sender.propagate_operations(to_send))
            .await
            .map_err(|err| ApiError::InternalServerError(err.to_string()))?
            .map_err(|err| {
                ApiError::InternalServerError(format!("Failed to propagate operations: {}", err))
            })?;
        Ok(ids)
    }

    /// Get events optionally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    async fn get_filtered_sc_output_event(
        &self,
        filter: EventFilter,
    ) -> RpcResult<Vec<SCOutputEvent>> {
        let events = self
            .0
            .execution_controller
            .get_filtered_sc_output_event(filter);

        // TODO: get rid of the async part
        Ok(events)
    }

    async fn node_peers_whitelist(&self) -> RpcResult<Vec<IpAddr>> {
        crate::wrong_api::<Vec<IpAddr>>()
    }

    async fn node_add_to_peers_whitelist(&self, _: Vec<IpAddr>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn node_remove_from_peers_whitelist(&self, _: Vec<IpAddr>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn node_bootstrap_whitelist(&self) -> RpcResult<Vec<IpAddr>> {
        crate::wrong_api::<Vec<IpAddr>>()
    }

    async fn node_bootstrap_whitelist_allow_all(&self) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn node_add_to_bootstrap_whitelist(&self, _: Vec<IpAddr>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn node_remove_from_bootstrap_whitelist(&self, _: Vec<IpAddr>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn node_bootstrap_blacklist(&self) -> RpcResult<Vec<IpAddr>> {
        crate::wrong_api::<Vec<IpAddr>>()
    }

    async fn node_add_to_bootstrap_blacklist(&self, _: Vec<IpAddr>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn node_remove_from_bootstrap_blacklist(&self, _: Vec<IpAddr>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn get_openrpc_spec(&self) -> RpcResult<Value> {
        let openrpc_spec_path = self.0.api_settings.openrpc_spec_path.clone();
        let openrpc: RpcResult<Value> = std::fs::read_to_string(openrpc_spec_path)
            .map_err(|e| {
                ApiError::InternalServerError(format!(
                    "failed to read OpenRPC specification: {}",
                    e
                ))
                .into()
            })
            .and_then(|openrpc_str| {
                serde_json::from_str(&openrpc_str).map_err(|e| {
                    ApiError::InternalServerError(format!(
                        "failed to parse OpenRPC specification: {}",
                        e
                    ))
                    .into()
                })
            });

        openrpc
    }
}
