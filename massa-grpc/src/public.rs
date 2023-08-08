// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaPublicGrpc;

use itertools::{izip, Itertools};
use massa_execution_exports::mapping_grpc::{
    to_event_filter, to_execution_query_response, to_querystate_filter,
};
use massa_execution_exports::{
    ExecutionQueryRequest, ExecutionStackElement, ReadOnlyExecutionRequest, ReadOnlyExecutionTarget,
};
use massa_models::address::Address;
use massa_models::block::{Block, BlockGraphStatus};
use massa_models::block_id::BlockId;
use massa_models::config::CompactConfig;
use massa_models::datastore::DatastoreDeserializer;
use massa_models::endorsement::{EndorsementId, SecureShareEndorsement};
use massa_models::operation::{OperationId, SecureShareOperation};
use massa_models::prehash::{PreHashMap, PreHashSet};
use massa_models::slot::Slot;
use massa_models::timeslots::get_latest_block_slot_at_timestamp;
use massa_proto_rs::massa::api::v1 as grpc_api;
use massa_proto_rs::massa::model::v1::{self as grpc_model, read_only_execution_call};
use massa_serialization::{DeserializeError, Deserializer};
use massa_time::MassaTime;
use massa_versioning::versioning_factory::{FactoryStrategy, VersioningFactory};
use std::collections::HashSet;
use std::str::FromStr;
use tracing::log::warn;

/// Execute read only call (function or bytecode)
pub(crate) fn execute_read_only_call(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::ExecuteReadOnlyCallRequest>,
) -> Result<grpc_api::ExecuteReadOnlyCallResponse, GrpcError> {
    let inner_req = request.into_inner();
    let call: grpc_model::ReadOnlyExecutionCall = inner_req
        .call
        .ok_or_else(|| GrpcError::InvalidArgument("no call provided".to_string()))?;

    let mut call_stack = Vec::new();

    let caller_address = match call.caller_address {
        Some(addr) => Address::from_str(&addr)?,
        None => {
            let now = MassaTime::now()?;
            let keypair = grpc.keypair_factory.create(&(), FactoryStrategy::At(now))?;
            Address::from_public_key(&keypair.get_public_key())
        }
    };

    let target = if let Some(call_target) = call.target {
        match call_target {
            read_only_execution_call::Target::BytecodeCall(value) => {
                let op_datastore = if value.operation_datastore.is_empty() {
                    None
                } else {
                    let deserializer = DatastoreDeserializer::new(
                        grpc.grpc_config.max_op_datastore_entry_count,
                        grpc.grpc_config.max_op_datastore_key_length,
                        grpc.grpc_config.max_op_datastore_value_length,
                    );
                    match deserializer.deserialize::<DeserializeError>(&value.operation_datastore) {
                        Ok((_, deserialized)) => Some(deserialized),
                        Err(e) => {
                            return Err(GrpcError::InvalidArgument(format!(
                                "Datastore deserializing error: {}",
                                e
                            )))
                        }
                    }
                };

                call_stack.push(ExecutionStackElement {
                    address: caller_address,
                    coins: Default::default(),
                    owned_addresses: vec![caller_address],
                    operation_datastore: op_datastore,
                });

                ReadOnlyExecutionTarget::BytecodeExecution(value.bytecode)
            }
            read_only_execution_call::Target::FunctionCall(value) => {
                let target_address = Address::from_str(&value.target_addr)?;
                call_stack.push(ExecutionStackElement {
                    address: caller_address,
                    coins: Default::default(),
                    owned_addresses: vec![caller_address],
                    operation_datastore: None, // should always be None
                });
                call_stack.push(ExecutionStackElement {
                    address: target_address,
                    coins: Default::default(),
                    owned_addresses: vec![target_address],
                    operation_datastore: None, // should always be None
                });

                ReadOnlyExecutionTarget::FunctionCall {
                    target_addr: Address::from_str(&value.target_addr)?,
                    target_func: value.target_func,
                    parameter: value.parameter,
                }
            }
        }
    } else {
        return Err(GrpcError::InvalidArgument(
            "no call target provided".to_string(),
        ));
    };

    let read_only_call = ReadOnlyExecutionRequest {
        max_gas: call.max_gas,
        call_stack,
        target,
        is_final: call.is_final,
    };

    let output = grpc
        .execution_controller
        .execute_readonly_request(read_only_call)?;

    let result = grpc_model::ReadOnlyExecutionOutput {
        out: Some(output.out.into()),
        used_gas: output.gas_cost,
        call_result: output.call_result,
    };

    Ok(grpc_api::ExecuteReadOnlyCallResponse {
        output: Some(result),
    })
}

/// Get blocks
pub(crate) fn get_blocks(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetBlocksRequest>,
) -> Result<grpc_api::GetBlocksResponse, GrpcError> {
    let block_ids = request.into_inner().block_ids;

    if block_ids.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no block id provided".to_string(),
        ));
    }

    if block_ids.len() as u32 > grpc.grpc_config.max_operation_ids_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many block ids received. Only a maximum of {} block ids are accepted per request",
            grpc.grpc_config.max_block_ids_per_request
        )));
    }

    let block_ids: Vec<BlockId> = block_ids
        .into_iter()
        .take(grpc.grpc_config.max_operation_ids_per_request as usize + 1)
        .map(|id| {
            BlockId::from_str(id.as_str())
                .map_err(|_| GrpcError::InvalidArgument(format!("invalid block id: {}", id)))
        })
        .collect::<Result<_, _>>()?;

    let read_blocks = grpc.storage.read_blocks();
    let blocks = block_ids
        .into_iter()
        .filter_map(|id| {
            let content = if let Some(wrapped_block) = read_blocks.get(&id) {
                wrapped_block.content.clone()
            } else {
                return None;
            };

            Some(content)
        })
        .collect::<Vec<Block>>();

    let block_ids = blocks
        .iter()
        .map(|block| block.header.id)
        .collect::<Vec<BlockId>>();

    let blocks_status = grpc.consensus_controller.get_block_statuses(&block_ids);

    let result = blocks
        .iter()
        .zip(blocks_status)
        .map(|(block, block_graph_status)| grpc_model::BlockWrapper {
            block: Some(block.clone().into()),
            status: block_graph_status.into(),
        })
        .collect();

    Ok(grpc_api::GetBlocksResponse {
        wrapped_blocks: result,
    })
}

/// Get multiple datastore entries
pub(crate) fn get_datastore_entries(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetDatastoreEntriesRequest>,
) -> Result<grpc_api::GetDatastoreEntriesResponse, GrpcError> {
    let inner_req = request.into_inner();

    let filters: Vec<(Address, Vec<u8>)> = inner_req
        .filters
        .into_iter()
        .filter_map(|filter| {
            filter.filter.and_then(|filter| match filter {
                grpc_api::get_datastore_entry_filter::Filter::AddressKey(addrs) => {
                    if let Ok(add) = &Address::from_str(&addrs.address) {
                        Some((*add, addrs.key))
                    } else {
                        None
                    }
                }
            })
        })
        .collect();

    // return error if entry are empty
    if filters.is_empty() {
        return Err(GrpcError::InvalidArgument("no filter provided".to_string()));
    }

    let entries = grpc
        .execution_controller
        .get_final_and_active_data_entry(filters)
        .into_iter()
        .map(|output| grpc_model::DatastoreEntry {
            final_value: output.0.unwrap_or_default(),
            candidate_value: output.1.unwrap_or_default(),
        })
        .collect();

    Ok(grpc_api::GetDatastoreEntriesResponse {
        datastore_entries: entries,
    })
}

/// Get endorsements
pub(crate) fn get_endorsements(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetEndorsementsRequest>,
) -> Result<grpc_api::GetEndorsementsResponse, GrpcError> {
    let endorsement_ids = request.into_inner().endorsement_ids;

    if endorsement_ids.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no endorsement id provided".to_string(),
        ));
    }

    //TODO to be replaced with a config value
    if endorsement_ids.len() as u32 > grpc.grpc_config.max_endorsements_per_message {
        return Err(GrpcError::InvalidArgument(format!(
            "too many endorsement ids received. Only a maximum of {} endorsement ids are accepted per request",
            grpc.grpc_config.max_endorsements_per_message
        )));
    }

    let endorsement_ids: Vec<EndorsementId> = endorsement_ids
        .into_iter()
        .take(grpc.grpc_config.max_operation_ids_per_request as usize + 1)
        .map(|id| {
            EndorsementId::from_str(id.as_str())
                .map_err(|_| GrpcError::InvalidArgument(format!("invalid endorsement id: {}", id)))
        })
        .collect::<Result<_, _>>()?;

    let storage_info: Vec<(SecureShareEndorsement, PreHashSet<BlockId>)> = {
        let read_blocks = grpc.storage.read_blocks();
        let read_endos = grpc.storage.read_endorsements();
        endorsement_ids
            .iter()
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
    let in_pool = grpc.pool_controller.contains_endorsements(&eds);

    let consensus_controller = grpc.consensus_controller.clone();

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
    let mut res: Vec<grpc_model::EndorsementWrapper> = Vec::with_capacity(eds.len());
    let zipped_iterator = izip!(
        storage_info.into_iter(),
        in_pool.into_iter(),
        is_final.into_iter()
    );
    for ((endorsement, in_blocks), in_pool, is_final) in zipped_iterator {
        res.push(grpc_model::EndorsementWrapper {
            in_pool,
            is_final,
            in_blocks: in_blocks
                .into_iter()
                .map(|block_id| block_id.to_string())
                .collect(),
            endorsement: Some(endorsement.into()),
        });
    }

    Ok(grpc_api::GetEndorsementsResponse {
        wrapped_endorsements: res,
    })
}

/// Get the stakers
pub(crate) fn get_stakers(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetStakersRequest>,
) -> Result<grpc_api::GetStakersResponse, GrpcError> {
    let inner_req = request.into_inner();

    // min_roll, max_roll, limit
    let mut filter_opt = (None, None, None);

    // Parse the query parameters, if provided.
    inner_req
        .filters
        .iter()
        .for_each(|filter| match filter.filter {
            Some(grpc_api::stakers_filter::Filter::MinRolls(min_rolls)) => {
                filter_opt.0 = Some(min_rolls);
            }
            Some(grpc_api::stakers_filter::Filter::MaxRolls(max_rolls)) => {
                filter_opt.1 = Some(max_rolls);
            }
            Some(grpc_api::stakers_filter::Filter::Limit(limit)) => {
                filter_opt.2 = Some(limit);
            }
            None => {}
        });

    // Get the current cycle and slot.
    let now: MassaTime = MassaTime::now()?;

    let latest_block_slot_at_timestamp_result = get_latest_block_slot_at_timestamp(
        grpc.grpc_config.thread_count,
        grpc.grpc_config.t0,
        grpc.grpc_config.genesis_timestamp,
        now,
    );

    let (cur_cycle, _cur_slot) = match latest_block_slot_at_timestamp_result {
        Ok(Some(cur_slot)) if cur_slot.period <= grpc.grpc_config.last_start_period => (
            Slot::new(grpc.grpc_config.last_start_period, 0)
                .get_cycle(grpc.grpc_config.periods_per_cycle),
            cur_slot,
        ),
        Ok(Some(cur_slot)) => (
            cur_slot.get_cycle(grpc.grpc_config.periods_per_cycle),
            cur_slot,
        ),
        Ok(None) => (0, Slot::new(0, 0)),
        Err(e) => return Err(GrpcError::ModelsError(e)),
    };

    // Get the list of stakers, filtered by the specified minimum and maximum roll counts.
    let mut staker_vec = grpc
        .execution_controller
        .get_cycle_active_rolls(cur_cycle)
        .into_iter()
        .filter_map(|(addr, rolls)| {
            if let Some(min_rolls) = filter_opt.0 {
                if rolls < min_rolls {
                    return None;
                }
            }
            if let Some(max_rolls) = filter_opt.1 {
                if rolls > max_rolls {
                    return None;
                }
            }
            Some((addr.to_string(), rolls))
        })
        .collect::<Vec<(String, u64)>>();

    // Sort the stakers by their roll counts in descending order.
    staker_vec.sort_by_key(|&(_, roll_counts)| std::cmp::Reverse(roll_counts));

    if let Some(limit) = filter_opt.2 {
        staker_vec = staker_vec
            .into_iter()
            .take(limit as usize)
            .collect::<Vec<(String, u64)>>();
    }

    let stakers = staker_vec
        .into_iter()
        .map(|(address, rolls)| grpc_model::StakerEntry { address, rolls })
        .collect();

    Ok(grpc_api::GetStakersResponse { stakers })
}

/// Get next block best parents
pub(crate) fn get_next_block_best_parents(
    grpc: &MassaPublicGrpc,
    _request: tonic::Request<grpc_api::GetNextBlockBestParentsRequest>,
) -> Result<grpc_api::GetNextBlockBestParentsResponse, GrpcError> {
    let block_parents = grpc
        .consensus_controller
        .get_best_parents()
        .into_iter()
        .map(|p| grpc_model::BlockParent {
            block_id: p.0.to_string(),
            period: p.1,
        })
        .collect();
    Ok(grpc_api::GetNextBlockBestParentsResponse { block_parents })
}

/// Get operations
pub(crate) fn get_operations(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetOperationsRequest>,
) -> Result<grpc_api::GetOperationsResponse, GrpcError> {
    let operation_ids = request.into_inner().operation_ids;

    if operation_ids.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no operations ids specified".to_string(),
        ));
    }

    if operation_ids.len() as u32 > grpc.grpc_config.max_operation_ids_per_request {
        return Err(GrpcError::InvalidArgument(format!("too many operations received. Only a maximum of {} operations are accepted per request", grpc.grpc_config.max_operation_ids_per_request)));
    }

    let operation_ids: Vec<OperationId> = operation_ids
        .into_iter()
        .take(grpc.grpc_config.max_operation_ids_per_request as usize + 1)
        .map(|id| {
            OperationId::from_str(id.as_str())
                .map_err(|_| GrpcError::InvalidArgument(format!("invalid operation id: {}", id)))
        })
        .collect::<Result<_, _>>()?;

    let read_blocks = grpc.storage.read_blocks();
    let read_ops = grpc.storage.read_operations();

    // Get the operations and the list of blocks that contain them from storage
    let storage_info: Vec<(&SecureShareOperation, HashSet<BlockId>)> = operation_ids
        .into_iter()
        .filter_map(|ope_id| {
            read_ops.get(&ope_id).map(|secure_share| {
                let block_ids = read_blocks
                    .get_blocks_by_operation(&ope_id)
                    .map(|hashset| hashset.iter().cloned().collect::<HashSet<BlockId>>())
                    .unwrap_or_default();

                (secure_share, block_ids)
            })
        })
        .collect();

    let operations: Vec<grpc_model::OperationWrapper> = storage_info
        .into_iter()
        .map(|secure_share| {
            let (secure_share, block_ids) = secure_share;
            grpc_model::OperationWrapper {
                thread: secure_share
                    .content_creator_address
                    .get_thread(grpc.grpc_config.thread_count) as u32,
                operation: Some((*secure_share).clone().into()),
                block_ids: block_ids.into_iter().map(|id| id.to_string()).collect(),
            }
        })
        .collect();

    Ok(grpc_api::GetOperationsResponse {
        wrapped_operations: operations,
    })
}

/// Get smart contract execution events
pub(crate) fn get_sc_execution_events(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetScExecutionEventsRequest>,
) -> Result<grpc_api::GetScExecutionEventsResponse, GrpcError> {
    let event_filter = to_event_filter(request.into_inner().filters)?;
    let events: Vec<grpc_model::ScExecutionEvent> = grpc
        .execution_controller
        .get_filtered_sc_output_event(event_filter)
        .into_iter()
        .map(|event| event.into())
        .collect();

    Ok(grpc_api::GetScExecutionEventsResponse { events })
}

//  Get selector draws
pub(crate) fn get_selector_draws(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetSelectorDrawsRequest>,
) -> Result<grpc_api::GetSelectorDrawsResponse, GrpcError> {
    let inner_req = request.into_inner();
    let mut addresses: PreHashSet<Address> = PreHashSet::default();
    let mut slot_range: (Option<Slot>, Option<Slot>) = (None, None);

    // parse filters from request
    inner_req.filters.into_iter().for_each(|query| {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::selector_draws_filter::Filter::Addresses(addrs) => {
                    addrs
                        .addresses
                        .into_iter()
                        .for_each(|addr| match Address::from_str(&addr) {
                            Ok(ad) => {
                                addresses.insert(ad);
                            }
                            Err(e) => warn!("failed to parse address: {}", e),
                        });
                }
                grpc_api::selector_draws_filter::Filter::SlotRange(range) => {
                    if let Some(start_slot) = range.start_slot {
                        slot_range.0 = Some(start_slot.into());
                    }
                    if let Some(end_slot) = range.end_slot {
                        slot_range.1 = Some(end_slot.into());
                    }
                }
            }
        }
    });

    if slot_range.0.is_none() || slot_range.1.is_none() {
        return Err(GrpcError::InvalidArgument(
            "slot range is required".to_string(),
        ));
    }

    // get future draws from selector
    let selection_draws = {
        let slot_start = slot_range.0.unwrap();
        let slot_end = slot_range.1.unwrap();
        let restrict_to_addresses = if addresses.is_empty() {
            None
        } else {
            Some(&addresses)
        };

        grpc.selector_controller
            .get_available_selections_in_range(slot_start..=slot_end, restrict_to_addresses)
            .unwrap_or_default()
            .into_iter()
            .map(|(v_slot, v_sel)| {
                let block_producer: Option<String> = if addresses.contains(&v_sel.producer) {
                    Some(v_sel.producer.to_string())
                } else {
                    None
                };
                let endorsement_producers: Vec<grpc_model::EndorsementDraw> = v_sel
                    .endorsements
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, endo_sel)| {
                        if addresses.contains(&endo_sel) {
                            Some(grpc_model::EndorsementDraw {
                                index: index as u64,
                                producer: endo_sel.to_string(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();

                grpc_model::SlotDraw {
                    slot: Some(v_slot.into()),
                    block_producer,
                    endorsement_draws: endorsement_producers,
                }
            })
            .collect()
    };

    Ok(grpc_api::GetSelectorDrawsResponse {
        draws: selection_draws,
    })
}

//  Get status
pub(crate) fn get_status(
    grpc: &MassaPublicGrpc,
    _request: tonic::Request<grpc_api::GetStatusRequest>,
) -> Result<grpc_api::GetStatusResponse, GrpcError> {
    let config = CompactConfig::default();
    let now = MassaTime::now()?;
    let last_slot = get_latest_block_slot_at_timestamp(
        grpc.grpc_config.thread_count,
        grpc.grpc_config.t0,
        grpc.grpc_config.genesis_timestamp,
        now,
    )?;

    let current_cycle = last_slot
        .unwrap_or_else(|| Slot::new(0, 0))
        .get_cycle(grpc.grpc_config.periods_per_cycle);
    let cycle_duration = grpc
        .grpc_config
        .t0
        .checked_mul(grpc.grpc_config.periods_per_cycle)?;
    let current_cycle_time = if current_cycle == 0 {
        grpc.grpc_config.genesis_timestamp
    } else {
        cycle_duration
            .checked_mul(current_cycle)
            .and_then(|elapsed_time_before_current_cycle| {
                grpc.grpc_config
                    .genesis_timestamp
                    .checked_add(elapsed_time_before_current_cycle)
            })?
    };
    let next_cycle_time = current_cycle_time.checked_add(cycle_duration)?;
    //TODO to be enhanced
    let empty_request = ExecutionQueryRequest { requests: vec![] };
    let state = grpc.execution_controller.query_state(empty_request);

    let status = grpc_model::PublicStatus {
        node_id: grpc.node_id.to_string(),
        version: grpc.version.to_string(),
        current_time: Some(now.into()),
        current_cycle,
        current_cycle_time: Some(current_cycle_time.into()),
        next_cycle_time: Some(next_cycle_time.into()),
        last_executed_final_slot: Some(state.final_cursor.into()),
        last_executed_speculative_slot: Some(state.candidate_cursor.into()),
        final_state_fingerprint: state.final_state_fingerprint.to_string(),
        config: Some(config.into()),
    };

    Ok(grpc_api::GetStatusResponse {
        status: Some(status),
    })
}

/// Get transactions throughput
pub(crate) fn get_transactions_throughput(
    grpc: &MassaPublicGrpc,
    _request: tonic::Request<grpc_api::GetTransactionsThroughputRequest>,
) -> Result<grpc_api::GetTransactionsThroughputResponse, GrpcError> {
    let stats = grpc.execution_controller.get_stats();
    let nb_sec_range = stats
        .time_window_end
        .saturating_sub(stats.time_window_start)
        .to_duration()
        .as_secs();

    let throughput = stats
        .final_executed_operations_count
        .checked_div(nb_sec_range as usize)
        .unwrap_or_default() as u32;

    Ok(grpc_api::GetTransactionsThroughputResponse { throughput })
}

/// Get query state
pub(crate) fn query_state(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::QueryStateRequest>,
) -> Result<grpc_api::QueryStateResponse, GrpcError> {
    let queries = request
        .into_inner()
        .queries
        .into_iter()
        .map(to_querystate_filter)
        .collect::<Result<Vec<_>, _>>()?;

    let response = grpc
        .execution_controller
        .query_state(ExecutionQueryRequest { requests: queries });

    Ok(grpc_api::QueryStateResponse {
        final_cursor: Some(response.final_cursor.into()),
        candidate_cursor: Some(response.candidate_cursor.into()),
        final_state_fingerprint: response.final_state_fingerprint.to_string(),
        responses: response
            .responses
            .into_iter()
            .map(to_execution_query_response)
            .collect(),
    })
}

/// Search blocks
pub(crate) fn search_blocks(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::SearchBlocksRequest>,
) -> Result<grpc_api::SearchBlocksResponse, GrpcError> {
    let inner_req = request.into_inner();

    let mut block_ids: Vec<BlockId> = Vec::new();
    let mut addresses: Option<Vec<String>> = None;
    let (mut slot_min, mut slot_max) = (None, None);

    // Get params filter from the request.
    for query in inner_req.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::search_blocks_filter::Filter::Addresses(addrs) => {
                    for addr in addrs.addresses {
                        if let Some(ref mut vec) = addresses {
                            vec.push(addr);
                        } else {
                            addresses = Some(vec![addr]);
                        }
                    }
                }
                grpc_api::search_blocks_filter::Filter::BlockIds(ids) => {
                    for id in ids.block_ids {
                        if block_ids.len() < grpc.grpc_config.max_block_ids_per_request as usize + 1
                        {
                            block_ids.push(BlockId::from_str(&id).map_err(|_| {
                                GrpcError::InvalidArgument(format!("invalid block id: {}", id))
                            })?);
                        }
                    }
                }
                grpc_api::search_blocks_filter::Filter::SlotRange(slot_range) => {
                    slot_max = slot_range.start_slot;
                    slot_min = slot_range.end_slot;
                }
            }
        }
    }

    // if no filter provided return an error
    if block_ids.is_empty() && addresses.is_none() && slot_min.is_none() && slot_max.is_none() {
        return Err(GrpcError::InvalidArgument("no filter provided".to_string()));
    }

    let read_blocks = grpc.storage.read_blocks();

    let blocks = if !block_ids.is_empty() {
        if block_ids.len() as u32 > grpc.grpc_config.max_block_ids_per_request {
            return Err(GrpcError::InvalidArgument(format!(
                "too many block ids received. Only a maximum of {} block ids are accepted per request",
                grpc.grpc_config.max_block_ids_per_request
            )));
        }

        block_ids
            .into_iter()
            .filter_map(|id| {
                let content = if let Some(wrapped_block) = read_blocks.get(&id) {
                    wrapped_block.content.clone()
                } else {
                    return None;
                };

                // check addresses filter
                if let Some(filter_addresses) = &addresses {
                    if !filter_addresses
                        .iter()
                        .any(|addr| content.header.content_creator_address.to_string().eq(addr))
                    {
                        return None;
                    }
                }
                // check slot filter
                if let Some(slot_min) = &slot_min {
                    if content.header.content.slot < slot_min.clone().into() {
                        return None;
                    }
                }
                if let Some(slot_max) = &slot_max {
                    if content.header.content.slot > slot_max.clone().into() {
                        return None;
                    }
                }

                Some(content)
            })
            .collect::<Vec<Block>>()
    } else if let Some(addresses) = addresses {
        let mut blocks = Vec::new();
        for addr in addresses.into_iter() {
            let address = Address::from_str(&addr)
                .map_err(|_| GrpcError::InvalidArgument(format!("invalid address: {}", addr)))?;
            if let Some(hash_set) = read_blocks.get_blocks_created_by(&address) {
                let result = hash_set
                    .iter()
                    .filter_map(|block_id| {
                        if let Some(block) = read_blocks
                            .get(block_id)
                            .map(|wrapped_block| wrapped_block.content.clone())
                        {
                            // check slot filter
                            if let Some(slot_min) = &slot_min {
                                if block.header.content.slot < slot_min.clone().into() {
                                    return None;
                                }
                            }
                            if let Some(slot_max) = &slot_max {
                                if block.header.content.slot > slot_max.clone().into() {
                                    return None;
                                }
                            }

                            return Some(block);
                        }

                        None
                    })
                    .collect::<Vec<Block>>();

                blocks.extend_from_slice(&result);
            }
        }
        blocks
    } else {
        // only slot range is provided
        let graph = grpc
            .consensus_controller
            .get_block_graph_status(slot_min.map(|s| s.into()), slot_max.map(|s| s.into()))?;

        graph
            .active_blocks
            .iter()
            .filter_map(|b| {
                read_blocks
                    .get(b.0)
                    .map(|wrapped_block| wrapped_block.content.clone())
            })
            .collect::<Vec<Block>>()
    };

    let block_ids = blocks
        .iter()
        .map(|block| block.header.id)
        .collect::<Vec<BlockId>>();

    let blocks_status = grpc.consensus_controller.get_block_statuses(&block_ids);

    let result = blocks
        .iter()
        .zip(blocks_status)
        .map(|(block, block_graph_status)| grpc_model::BlockInfo {
            block_id: block.header.id.to_string(),
            status: block_graph_status.into(),
        })
        .collect();

    Ok(grpc_api::SearchBlocksResponse {
        block_infos: result,
    })
}

/// Search operations
pub(crate) fn search_operations(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::SearchOperationsRequest>,
) -> Result<grpc_api::SearchOperationsResponse, GrpcError> {
    let inner_req: grpc_api::SearchOperationsRequest = request.into_inner();

    let mut operation_ids = Vec::new();
    let mut filter_ope_types = Vec::new();

    // Get params filter from the request.
    for query in inner_req.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::search_operations_filter::Filter::OperationIds(ids) => {
                    for id in ids.operation_ids {
                        if operation_ids.len()
                            < grpc.grpc_config.max_operation_ids_per_request as usize + 1
                        {
                            operation_ids.push(OperationId::from_str(&id).map_err(|_| {
                                GrpcError::InvalidArgument(format!("invalid operation id: {}", id))
                            })?);
                        }
                    }
                }
                grpc_api::search_operations_filter::Filter::OperationTypes(ope_types) => {
                    filter_ope_types.extend_from_slice(&ope_types.op_types);
                }
            }
        }
    }

    if operation_ids.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no operations ids specified".to_string(),
        ));
    }

    if operation_ids.len() as u32 > grpc.grpc_config.max_operation_ids_per_request {
        return Err(GrpcError::InvalidArgument(format!("too many operations received. Only a maximum of {} operations are accepted per request", grpc.grpc_config.max_operation_ids_per_request)));
    }

    let read_blocks = grpc.storage.read_blocks();
    let read_ops = grpc.storage.read_operations();

    // Get the operations and the list of blocks that contain them from storage
    let storage_info: Vec<(&SecureShareOperation, HashSet<BlockId>)> = operation_ids
        .iter()
        .filter_map(|ope_id| {
            read_ops.get(ope_id).map(|secure_share| {
                let block_ids = read_blocks
                    .get_blocks_by_operation(ope_id)
                    .map(|hashset| hashset.iter().cloned().collect::<HashSet<BlockId>>())
                    .unwrap_or_default();

                (secure_share, block_ids)
            })
        })
        .collect();

    let operations: Vec<grpc_model::OperationInfo> = storage_info
        .into_iter()
        .filter_map(|secure_share| {
            let (secure_share, block_ids) = secure_share;
            let ope_type: grpc_model::OpType = secure_share.content.op.clone().into();
            if !filter_ope_types.is_empty() && !filter_ope_types.contains(&(ope_type as i32)) {
                return None;
            }

            Some(grpc_model::OperationInfo {
                id: secure_share.id.to_string(),
                thread: secure_share
                    .content_creator_address
                    .get_thread(grpc.grpc_config.thread_count) as u32,
                block_ids: block_ids.into_iter().map(|id| id.to_string()).collect(),
            })
        })
        .collect();

    Ok(grpc_api::SearchOperationsResponse {
        operation_infos: operations,
    })
}
