//! Copyright (c) 2022 MASSA LABS <info@massa.net>
#![allow(clippy::too_many_arguments)]

use crate::{MassaRpcServer, Public, RpcServer, StopHandle, Value, API};
use async_trait::async_trait;
use itertools::{izip, Itertools};
use jsonrpsee::core::{Error as JsonRpseeError, RpcResult};
use massa_api_exports::{
    address::{AddressFilter, AddressInfo},
    block::{BlockInfo, BlockInfoContent, BlockSummary},
    config::APIConfig,
    datastore::{DatastoreEntryInput, DatastoreEntryOutput},
    endorsement::EndorsementInfo,
    error::ApiError,
    execution::{
        ExecuteReadOnlyResponse, ReadOnlyBytecodeExecution, ReadOnlyCall, ReadOnlyResult, Transfer,
    },
    node::NodeStatus,
    operation::{OperationInfo, OperationInput},
    page::{PageRequest, PagedVec},
    slot::SlotAmount,
    TimeInterval,
};
use massa_consensus_exports::block_status::DiscardReason;
use massa_consensus_exports::ConsensusController;
use massa_execution_exports::{
    ExecutionController, ExecutionQueryRequest, ExecutionQueryRequestItem,
    ExecutionQueryResponseItem, ExecutionStackElement, ReadOnlyExecutionRequest,
    ReadOnlyExecutionTarget,
};
use massa_models::{
    address::Address,
    amount::Amount,
    block::{Block, BlockGraphStatus},
    block_id::BlockId,
    clique::Clique,
    composite::PubkeySig,
    config::CompactConfig,
    datastore::DatastoreDeserializer,
    endorsement::EndorsementId,
    endorsement::SecureShareEndorsement,
    error::ModelsError,
    execution::EventFilter,
    node::NodeId,
    operation::OperationDeserializer,
    operation::OperationId,
    operation::{OperationType, SecureShareOperation},
    output_event::SCOutputEvent,
    prehash::{PreHashMap, PreHashSet},
    secure_share::SecureShareDeserializer,
    slot::{IndexedSlot, Slot},
    timeslots,
    timeslots::{get_latest_block_slot_at_timestamp, time_range_to_slot_range},
    version::Version,
};
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::{PeerConnectionType, ProtocolConfig, ProtocolController};
use massa_serialization::{DeserializeError, Deserializer};
use massa_storage::Storage;
use massa_time::MassaTime;
use massa_versioning::versioning_factory::FactoryStrategy;
use massa_versioning::{
    keypair_factory::KeyPairFactory, versioning::MipStore, versioning_factory::VersioningFactory,
};
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
        protocol_controller: Box<dyn ProtocolController>,
        protocol_config: ProtocolConfig,
        version: Version,
        node_id: NodeId,
        storage: Storage,
        mip_store: MipStore,
    ) -> Self {
        API(Public {
            consensus_controller,
            api_settings,
            pool_command_sender,
            version,
            protocol_controller,
            node_id,
            execution_controller,
            selector_controller,
            protocol_config,
            storage,
            keypair_factory: KeyPairFactory { mip_store },
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
    fn stop_node(&self) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    async fn node_sign_message(&self, _: Vec<u8>) -> RpcResult<PubkeySig> {
        crate::wrong_api::<PubkeySig>()
    }

    async fn add_staking_secret_keys(&self, _: Vec<String>) -> RpcResult<()> {
        crate::wrong_api::<()>()
    }

    #[cfg(feature = "execution-trace")]
    async fn get_slots_transfers(&self, slots: Vec<Slot>) -> RpcResult<Vec<Vec<Transfer>>> {
        use massa_api_exports::execution::TransferContext;
        use std::str::FromStr;

        let mut res: Vec<Vec<Transfer>> = Vec::with_capacity(slots.len());
        for slot in slots {
            let Some(block_id) = self
                .0
                .consensus_controller
                .get_blockclique_block_at_slot(slot)
            else {
                continue;
            };
            let mut transfers = Vec::new();
            let abi_calls = self
                .0
                .execution_controller
                .get_slot_abi_call_stack(slot.clone().into());
            if let Some(abi_calls) = abi_calls {
                // flatten & filter transfer trace in asc_call_stacks

                let abi_transfer_1 = "assembly_script_transfer_coins".to_string();
                let abi_transfer_2 = "assembly_script_transfer_coins_for".to_string();
                let abi_transfer_3 = "abi_transfer_coins".to_string();
                let transfer_abi_names = vec![abi_transfer_1, abi_transfer_2, abi_transfer_3];
                for (i, asc_call_stack) in abi_calls.asc_call_stacks.iter().enumerate() {
                    for abi_trace in asc_call_stack {
                        let only_transfer = abi_trace.flatten_filter(&transfer_abi_names);
                        for transfer in only_transfer {
                            let (t_from, t_to, t_amount) = transfer.parse_transfer();
                            transfers.push(Transfer {
                                from: Address::from_str(&t_from).unwrap(),
                                to: Address::from_str(&t_to).unwrap(),
                                amount: Amount::from_raw(t_amount),
                                effective_amount_received: Amount::from_raw(t_amount),
                                context: TransferContext::ASC(i as u64),
                                succeed: true,
                                fee: Amount::from_raw(0),
                                block_id,
                            });
                        }
                    }
                }

                for op_call_stack in abi_calls.operation_call_stacks {
                    let op_id = op_call_stack.0;
                    let op_call_stack = op_call_stack.1;
                    for abi_trace in op_call_stack {
                        let only_transfer = abi_trace.flatten_filter(&transfer_abi_names);
                        for transfer in only_transfer {
                            let (t_from, t_to, t_amount) = transfer.parse_transfer();
                            transfers.push(Transfer {
                                from: Address::from_str(&t_from).unwrap(),
                                to: Address::from_str(&t_to).unwrap(),
                                amount: Amount::from_raw(t_amount),
                                effective_amount_received: Amount::from_raw(t_amount),
                                context: TransferContext::Operation(op_id),
                                succeed: true,
                                fee: Amount::from_raw(0),
                                block_id,
                            });
                        }
                    }
                }
            }
            let transfers_op: Vec<Transfer> = self
                .0
                .execution_controller
                .get_transfers_for_slot(slot)
                .unwrap_or_default()
                .iter()
                .map(|t| Transfer {
                    from: t.from,
                    to: t.to,
                    amount: t.amount,
                    effective_amount_received: t.effective_received_amount,
                    context: TransferContext::Operation(t.op_id),
                    succeed: t.succeed,
                    fee: t.fee,
                    block_id,
                })
                .collect();
            transfers.extend(transfers_op);
            res.push(transfers);
        }
        Ok(res)
    }

    #[cfg(not(feature = "execution-trace"))]
    async fn get_slots_transfers(&self, _: Vec<Slot>) -> RpcResult<Vec<Vec<Transfer>>> {
        RpcResult::Err(ApiError::BadRequest("feature execution-trace is not enabled".into()).into())
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
            fee,
        } in reqs
        {
            if address.is_none() && fee.is_some() {
                return Err(
                    ApiError::BadRequest("fee argument is set without address".into()).into(),
                );
            }
            let address = if let Some(addr) = address {
                addr
            } else {
                let now = MassaTime::now();
                let keypair = self
                    .0
                    .keypair_factory
                    .create(&(), FactoryStrategy::At(now))
                    .map_err(ApiError::from)?;
                Address::from_public_key(&keypair.get_public_key())
            };

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
                coins: None,
                fee,
            };

            // check if fee is enough
            if let Some(fee) = fee {
                if fee.checked_sub(self.0.api_settings.minimal_fees).is_none() {
                    let result = ExecuteReadOnlyResponse {
                        executed_at: Slot::new(0, 0),
                        result: ReadOnlyResult::Error(format!(
                            "fee is too low provided: {} , minimal_fees required: {}",
                            fee, self.0.api_settings.minimal_fees
                        )),
                        gas_cost: 0,
                        output_events: Default::default(),
                        state_changes: Default::default(),
                    };
                    res.push(result);
                    continue;
                }
            }

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

    /// execute read-only calls
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
            coins,
            fee,
        } in reqs
        {
            if caller_address.is_none() && (fee.is_some() || coins.is_some()) {
                return Err(ApiError::BadRequest(
                    "fee or coins argument is set without caller_address".into(),
                )
                .into());
            }
            let caller_address = if let Some(addr) = caller_address {
                addr
            } else {
                let now = MassaTime::now();
                let keypair = self
                    .0
                    .keypair_factory
                    .create(&(), FactoryStrategy::At(now))
                    .map_err(ApiError::from)?;
                Address::from_public_key(&keypair.get_public_key())
            };

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
                        coins: coins.unwrap_or(Amount::default()),
                        owned_addresses: vec![target_address],
                        operation_datastore: None, // should always be None
                    },
                ],
                coins,
                fee,
            };

            if let Some(fee) = fee {
                if fee.checked_sub(self.0.api_settings.minimal_fees).is_none() {
                    let result = ExecuteReadOnlyResponse {
                        executed_at: Slot::new(0, 0),
                        result: ReadOnlyResult::Error(format!(
                            "fee is too low provided: {} , minimal_fees required: {}",
                            fee, self.0.api_settings.minimal_fees
                        )),
                        gas_cost: 0,
                        output_events: Default::default(),
                        state_changes: Default::default(),
                    };
                    res.push(result);
                    continue;
                }
            }

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

    /// get status
    async fn get_status(&self) -> RpcResult<NodeStatus> {
        let version = self.0.version;
        let api_settings = self.0.api_settings.clone();
        let protocol_config = self.0.protocol_config.clone();
        let node_id = self.0.node_id;
        let config = CompactConfig::default();
        let now = MassaTime::now();

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

        let execution_stats = self.0.execution_controller.get_stats();
        let consensus_stats_result = self.0.consensus_controller.get_stats();
        let consensus_stats = match consensus_stats_result {
            Ok(consensus_stats) => consensus_stats,
            Err(e) => return Err(ApiError::ConsensusError(e.to_string()).into()),
        };

        let (network_stats, peers) = match self.0.protocol_controller.get_stats() {
            Ok((stats, peers)) => (stats, peers),
            Err(e) => return Err(ApiError::ProtocolError(e.to_string()).into()),
        };

        let pool_stats = (
            self.0.pool_command_sender.get_operation_count(),
            self.0.pool_command_sender.get_endorsement_count(),
        );

        let next_slot_result = last_slot
            .unwrap_or_else(|| Slot::new(0, 0))
            .get_next_slot(api_settings.thread_count);

        let next_slot = match next_slot_result {
            Ok(next_slot) => next_slot,
            Err(e) => return Err(ApiError::ModelsError(e).into()),
        };

        let connected_nodes = peers
            .iter()
            .map(|(id, peer)| {
                let is_outgoing = match peer.1 {
                    PeerConnectionType::IN => false,
                    PeerConnectionType::OUT => true,
                };
                (NodeId::new(id.get_public_key()), (peer.0.ip(), is_outgoing))
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
            node_ip: protocol_config.routable_ip,
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
            chain_id: self.0.api_settings.chain_id,
            minimal_fees: self.0.api_settings.minimal_fees,
        })
    }

    /// get cliques
    async fn get_cliques(&self) -> RpcResult<Vec<Clique>> {
        Ok(self.0.consensus_controller.get_cliques())
    }

    /// get stakers
    async fn get_stakers(
        &self,
        page_request: Option<PageRequest>,
    ) -> RpcResult<PagedVec<(Address, u64)>> {
        let cfg = self.0.api_settings.clone();

        let now = MassaTime::now();

        let latest_block_slot_at_timestamp_result = get_latest_block_slot_at_timestamp(
            cfg.thread_count,
            cfg.t0,
            cfg.genesis_timestamp,
            now,
        );

        let curr_cycle = match latest_block_slot_at_timestamp_result {
            Ok(Some(cur_slot)) if cur_slot.period <= self.0.api_settings.last_start_period => {
                Slot::new(self.0.api_settings.last_start_period, 0).get_cycle(cfg.periods_per_cycle)
            }
            Ok(Some(cur_slot)) => cur_slot.get_cycle(cfg.periods_per_cycle),
            Ok(None) => 0,
            Err(e) => return Err(ApiError::ModelsError(e).into()),
        };

        let mut staker_vec = self
            .0
            .execution_controller
            .get_cycle_active_rolls(curr_cycle)
            .into_iter()
            .collect::<Vec<(Address, u64)>>();

        staker_vec
            .sort_by(|&(_, roll_counts_a), &(_, roll_counts_b)| roll_counts_b.cmp(&roll_counts_a));

        let paged_vec = PagedVec::new(staker_vec, page_request);

        Ok(paged_vec)
    }

    /// get operations
    async fn get_operations(
        &self,
        operations_ids: Vec<OperationId>,
    ) -> RpcResult<Vec<OperationInfo>> {
        // get the operations and the list of blocks that contain them from storage
        let secure_share_operations: Vec<SecureShareOperation> = {
            let read_ops = self.0.storage.read_operations();
            operations_ids
                .iter()
                .filter_map(|id| read_ops.get(id).cloned())
                .collect()
        };

        let storage_info: Vec<(SecureShareOperation, PreHashSet<BlockId>)> = {
            let read_blocks = self.0.storage.read_blocks();
            secure_share_operations
                .into_iter()
                .map(|secure_share_operation| {
                    let op_id = secure_share_operation.id;
                    (
                        secure_share_operation,
                        read_blocks
                            .get_blocks_by_operation(&op_id)
                            .cloned()
                            .unwrap_or_default(),
                    )
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

        let op_exec_statuses = self.0.execution_controller.get_ops_exec_status(&ops);

        // compute operation finality and operation execution status from *_op_exec_statuses
        let (is_operation_final, statuses): (Vec<Option<bool>>, Vec<Option<bool>>) =
            op_exec_statuses
                .into_iter()
                .map(|(spec_exec, final_exec)| match (spec_exec, final_exec) {
                    (Some(true), Some(true)) => (Some(true), Some(true)),
                    (Some(false), Some(false)) => (Some(true), Some(false)),
                    (Some(true), None) => (Some(false), Some(true)),
                    (Some(false), None) => (Some(false), Some(false)),
                    _ => (None, None),
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
            #[cfg(feature = "execution-trace")]
            {
                let mut transfer = None;
                if is_operation_final.is_none() || op_exec_status.is_none() {
                    transfer = self.0.execution_controller.get_transfer_for_op(&id);
                }
                let is_operation_final = is_operation_final.or(Some(transfer.is_some()));
                let op_exec_status = op_exec_status.or(transfer.map(|t| t.succeed));
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
            #[cfg(not(feature = "execution-trace"))]
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
        }

        // return values in the right order
        Ok(res)
    }

    /// get endorsements
    async fn get_endorsements(
        &self,
        mut endorsement_ids: Vec<EndorsementId>,
    ) -> RpcResult<Vec<EndorsementInfo>> {
        if endorsement_ids.len() as u64 > self.0.api_settings.max_arguments {
            return Err(ApiError::BadRequest("too many arguments".into()).into());
        }

        let mut secure_share_endorsements: Vec<SecureShareEndorsement> =
            Vec::with_capacity(endorsement_ids.len());
        {
            let endorsement_storage_lock = self.0.storage.read_endorsements();
            endorsement_ids.retain(|id| {
                if let Some(wrapped_endorsement) = endorsement_storage_lock.get(id) {
                    secure_share_endorsements.push(wrapped_endorsement.clone());
                    return true;
                };
                false
            });
        }

        let storage_info: Vec<(SecureShareEndorsement, PreHashSet<BlockId>)> = {
            let read_blocks = self.0.storage.read_blocks();
            secure_share_endorsements
                .into_iter()
                .map(|secure_share_operation| {
                    let ed_id = secure_share_operation.id;
                    (
                        secure_share_operation,
                        read_blocks
                            .get_blocks_by_endorsement(&ed_id)
                            .cloned()
                            .unwrap_or_default(),
                    )
                })
                .collect()
        };

        // ask pool whether it carries the operations
        let in_pool = self
            .0
            .pool_command_sender
            .contains_endorsements(&endorsement_ids);

        // check finality by cross-referencing Consensus and looking for final blocks that contain the endorsement
        let is_final: Vec<bool> = {
            let involved_blocks: Vec<BlockId> = storage_info
                .iter()
                .flat_map(|(_ed, bs)| bs.iter())
                .unique()
                .cloned()
                .collect();

            let involved_block_statuses = self
                .0
                .consensus_controller
                .get_block_statuses(&involved_blocks);

            let block_statuses: PreHashMap<BlockId, BlockGraphStatus> = involved_blocks
                .into_iter()
                .zip(involved_block_statuses)
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
        let mut res: Vec<EndorsementInfo> = Vec::with_capacity(endorsement_ids.len());
        let zipped_iterator = izip!(
            endorsement_ids.into_iter(),
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

    /// get blocks
    /// Returns only active blocks are returned
    async fn get_blocks(&self, mut ids: Vec<BlockId>) -> RpcResult<Vec<BlockInfo>> {
        let mut blocks: Vec<Block> = Vec::with_capacity(ids.len());
        {
            let block_storage_lock = self.0.storage.read_blocks();
            ids.retain(|id| {
                if let Some(wrapped_block) = block_storage_lock.get(id) {
                    blocks.push(wrapped_block.content.clone());
                    return true;
                };
                false
            });
        }
        let block_statuses = self.0.consensus_controller.get_block_statuses(&ids);
        let res = ids
            .into_iter()
            .zip(blocks)
            .zip(block_statuses)
            .map(|((id, content), graph_status)| BlockInfo {
                id,
                content: Some(BlockInfoContent {
                    is_final: graph_status == BlockGraphStatus::Final,
                    is_in_blockclique: graph_status == BlockGraphStatus::ActiveInBlockclique,
                    is_candidate: graph_status == BlockGraphStatus::ActiveInBlockclique
                        || graph_status == BlockGraphStatus::ActiveInAlternativeCliques,
                    is_discarded: graph_status == BlockGraphStatus::Discarded,
                    block: content,
                }),
            })
            .collect();
        Ok(res)
    }

    async fn get_blockclique_block_by_slot(&self, slot: Slot) -> RpcResult<Option<Block>> {
        let block_id_option = self
            .0
            .consensus_controller
            .get_blockclique_block_at_slot(slot);

        let block_id = match block_id_option {
            Some(id) => id,
            None => return Ok(None),
        };

        let res = self
            .0
            .storage
            .read_blocks()
            .get(&block_id)
            .map(|b| b.content.clone());
        Ok(res)
    }

    /// gets an interval of the block graph from consensus, with time filtering
    /// time filtering is done consensus-side to prevent communication overhead
    async fn get_graph_interval(&self, time: TimeInterval) -> RpcResult<Vec<BlockSummary>> {
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

        let graph = match self
            .0
            .consensus_controller
            .get_block_graph_status(start_slot, end_slot)
        {
            Ok(graph) => graph,
            Err(e) => return Err(ApiError::ConsensusError(e.to_string()).into()),
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

    /// get datastore entries
    async fn get_datastore_entries(
        &self,
        entries: Vec<DatastoreEntryInput>,
    ) -> RpcResult<Vec<DatastoreEntryOutput>> {
        Ok(self
            .0
            .execution_controller
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

    /// get addresses
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

        // Compute a limit (as a slot) for deferred credits as it can be quite huge
        let bound_ts = MassaTime::now().saturating_add(self.0.api_settings.deferred_credits_delta);

        let deferred_credit_max_slot = timeslots::get_closest_slot_to_timestamp(
            self.0.api_settings.thread_count,
            self.0.api_settings.t0,
            self.0.api_settings.genesis_timestamp,
            bound_ts,
        );

        // get execution info
        let execution_infos = self.0.execution_controller.get_addresses_infos(
            &addresses,
            std::ops::Bound::Included(deferred_credit_max_slot),
        );

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
            let selections = self
                .0
                .selector_controller
                .get_available_selections_in_range(
                    cur_slot..=slot_end,
                    Some(&addresses.iter().copied().collect()),
                )
                .unwrap_or_default();

            addresses
                .iter()
                .map(|addr| {
                    let mut producer_slots = Vec::new();
                    let mut endorser_slots = Vec::new();
                    for (selection_slot, selection) in &selections {
                        if selection.producer == *addr {
                            producer_slots.push(*selection_slot);
                        }
                        for (index, endorser) in selection.endorsements.iter().enumerate() {
                            if endorser == addr {
                                endorser_slots.push(IndexedSlot {
                                    slot: *selection_slot,
                                    index,
                                });
                            }
                        }
                    }
                    (producer_slots, endorser_slots)
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

    /// get addresses bytecode
    async fn get_addresses_bytecode(&self, args: Vec<AddressFilter>) -> RpcResult<Vec<Vec<u8>>> {
        let queries = args
            .into_iter()
            .map(|arg| {
                if arg.is_final {
                    ExecutionQueryRequestItem::AddressBytecodeFinal(arg.address)
                } else {
                    ExecutionQueryRequestItem::AddressBytecodeCandidate(arg.address)
                }
            })
            .collect::<Vec<_>>();

        if queries.is_empty() {
            return Err(ApiError::BadRequest("no arguments specified".to_string()).into());
        }

        if queries.len() as u64 > self.0.api_settings.max_arguments {
            return Err(ApiError::BadRequest(format!("too many arguments received. Only a maximum of {} arguments are accepted per request", self.0.api_settings.max_arguments)).into());
        }

        let responses = self
            .0
            .execution_controller
            .query_state(ExecutionQueryRequest { requests: queries })
            .responses;

        let res: Result<Vec<Vec<u8>>, ApiError> = responses
            .into_iter()
            .map(|value| match value {
                Ok(item) => match item {
                    ExecutionQueryResponseItem::Bytecode(bytecode) => Ok(bytecode.0),
                    _ => Err(ApiError::InternalServerError(
                        "unexpected response type".to_string(),
                    )),
                },
                Err(err) => Err(ApiError::InternalServerError(err.to_string())),
            })
            .collect();

        Ok(res?)
    }

    /// send operations
    async fn send_operations(&self, ops: Vec<OperationInput>) -> RpcResult<Vec<OperationId>> {
        let mut cmd_sender = self.0.pool_command_sender.clone();
        let protocol_sender = self.0.protocol_controller.clone();
        let api_cfg = &self.0.api_settings;
        let mut to_send = self.0.storage.clone_without_refs();

        if ops.len() as u64 > api_cfg.max_arguments {
            return Err(ApiError::BadRequest("too many arguments".into()).into());
        }
        let now = MassaTime::now();
        let last_slot = get_latest_block_slot_at_timestamp(
            api_cfg.thread_count,
            api_cfg.t0,
            api_cfg.genesis_timestamp,
            now,
        )
        .map_err(ApiError::ModelsError)?;

        let verified_ops = ops
            .into_iter()
            .map(|op_input| check_input_operation(op_input, api_cfg, last_slot))
            .map(|op| match op {
                Ok(operation) => {
                    if operation
                        .content
                        .fee
                        .checked_sub(api_cfg.minimal_fees)
                        .is_none()
                    {
                        return Err(ApiError::BadRequest(format!(
                            "fee is too low provided: {} , minimal_fees required: {}",
                            operation.content.fee, self.0.api_settings.minimal_fees
                        ))
                        .into());
                    }

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

    /// Get the OpenRPC specification of the node
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

/// Checks the validity of an input operation.
///
/// This function takes an `OperationInput`, an `APIConfig`, and an optional `Slot` as input parameters.
/// It performs various checks on the input operation and returns a `SecureShareOperation` if the checks pass.
/// Otherwise, it returns an `RpcResult` with an appropriate error.
///
/// # Arguments
///
/// * `op_input` - The input operation to be checked.
/// * `api_cfg` - The API configuration used for checking the operation.
/// * `last_slot` - An optional `Slot` representing the last slot used.
///
/// # Returns
///
/// Returns a `RpcResult` containing a `SecureShareOperation` if the input operation is valid.
/// Otherwise, returns an `RpcResult` with an appropriate error.
fn check_input_operation(
    op_input: OperationInput,
    api_cfg: &APIConfig,
    last_slot: Option<Slot>,
) -> RpcResult<SecureShareOperation> {
    let operation_deserializer = SecureShareDeserializer::new(
        OperationDeserializer::new(
            api_cfg.max_datastore_value_length,
            api_cfg.max_function_name_length,
            api_cfg.max_parameter_size,
            api_cfg.max_op_datastore_entry_count,
            api_cfg.max_op_datastore_key_length,
            api_cfg.max_op_datastore_value_length,
        ),
        api_cfg.chain_id,
    );

    let mut op_serialized = Vec::new();
    op_serialized.extend(op_input.signature.to_bytes());
    op_serialized.extend(op_input.creator_public_key.to_bytes());
    op_serialized.extend(op_input.serialized_content);
    let (rest, op): (&[u8], SecureShareOperation) = operation_deserializer
        .deserialize::<DeserializeError>(&op_serialized)
        .map_err(|err| ApiError::ModelsError(ModelsError::DeserializeError(err.to_string())))?;
    match op.content.op {
        OperationType::CallSC { .. } => {
            let gas_usage =
                op.get_gas_usage(api_cfg.base_operation_gas_cost, api_cfg.sp_compilation_cost);
            if gas_usage > api_cfg.max_gas_per_block {
                let err_msg = format!("Upper gas limit for CallSC operation is {}. Your operation will never be included in a block.",
                    api_cfg.max_gas_per_block.saturating_sub(api_cfg.base_operation_gas_cost));
                return Err(ApiError::InconsistencyError(err_msg).into());
            }
        }
        OperationType::ExecuteSC { .. } => {
            let gas_usage =
                op.get_gas_usage(api_cfg.base_operation_gas_cost, api_cfg.sp_compilation_cost);
            if gas_usage > api_cfg.max_gas_per_block {
                let err_msg = format!("Upper gas limit for ExecuteSC operation is {}. Your operation will never be included in a block.",
                    api_cfg.max_gas_per_block.saturating_sub(api_cfg.base_operation_gas_cost).saturating_sub(api_cfg.sp_compilation_cost));
                return Err(ApiError::InconsistencyError(err_msg).into());
            }
        }
        _ => {}
    };
    if let Some(slot) = last_slot {
        if op.content.expire_period < slot.period {
            return Err(
                ApiError::InconsistencyError(
                    "Operation expire_period is lower than the current period of this node. Your operation will never be included in a block.".into()
                ).into()
            );
        }
    }
    if rest.is_empty() {
        Ok(op)
    } else {
        Err(ApiError::ModelsError(ModelsError::DeserializeError(
            "There is data left after operation deserialization".to_owned(),
        ))
        .into())
    }
}
