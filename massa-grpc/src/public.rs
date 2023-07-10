// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaPublicGrpc;

use massa_execution_exports::ExecutionQueryRequest;
use massa_models::address::Address;
use massa_models::block::Block;
use massa_models::block_id::BlockId;
use massa_models::config::CompactConfig;
use massa_models::execution::EventFilter;
use massa_models::operation::{OperationId, SecureShareOperation};
use massa_models::prehash::PreHashSet;
use massa_models::slot::Slot;
use massa_models::timeslots::get_latest_block_slot_at_timestamp;
use massa_proto_rs::massa::api::v1 as grpc_api;
use massa_proto_rs::massa::model::v1 as grpc_model;
use massa_time::MassaTime;
use std::collections::HashSet;
use std::str::FromStr;
use tracing::log::warn;

/// Get blocks
pub(crate) fn execute_read_only_call(
    _grpc: &MassaPublicGrpc,
    _request: tonic::Request<grpc_api::ExecuteReadOnlyCallRequest>,
) -> Result<grpc_api::ExecuteReadOnlyCallResponse, GrpcError> {
    Err(GrpcError::Unimplemented(
        "execute_read_only_call".to_string(),
    ))
}

/// Get blocks
pub(crate) fn get_blocks(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetBlocksRequest>,
) -> Result<grpc_api::GetBlocksResponse, GrpcError> {
    let inner_req = request.into_inner();

    let mut blocks_ids: Vec<BlockId> = Vec::new();
    let mut addresses: Option<Vec<String>> = None;
    let (mut slot_min, mut slot_max) = (None, None);

    // Get params filter from the request.
    for query in inner_req.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::get_blocks_filter::Filter::Addresses(addrs) => {
                    for addr in addrs.addresses {
                        if let Some(ref mut vec) = addresses {
                            vec.push(addr);
                        } else {
                            addresses = Some(vec![addr]);
                        }
                    }
                }
                grpc_api::get_blocks_filter::Filter::BlockIds(ids) => {
                    for id in ids.block_ids {
                        if blocks_ids.len()
                            < grpc.grpc_config.max_block_ids_per_request as usize + 1
                        {
                            blocks_ids.push(BlockId::from_str(&id).map_err(|_| {
                                GrpcError::InvalidArgument(format!("invalid block id: {}", id))
                            })?);
                        }
                    }
                }
                grpc_api::get_blocks_filter::Filter::SlotRange(slot_range) => {
                    slot_max = slot_range.start_slot;
                    slot_min = slot_range.end_slot;
                }
            }
        }
    }

    // if no filter provided return an error
    if blocks_ids.is_empty() && addresses.is_none() && slot_min.is_none() && slot_max.is_none() {
        return Err(GrpcError::InvalidArgument("no filter provided".to_string()));
    }

    let storage = grpc.storage.clone_without_refs();
    let read_blocks = storage.read_blocks();

    let blocks = if !blocks_ids.is_empty() {
        if blocks_ids.len() as u32 > grpc.grpc_config.max_block_ids_per_request {
            return Err(GrpcError::InvalidArgument(format!(
                "too many block ids received. Only a maximum of {} block ids are accepted per request",
                grpc.grpc_config.max_block_ids_per_request
            )));
        }

        blocks_ids
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
            if let Ok(address) = &Address::from_str(&addr)
                .map_err(|_| GrpcError::InvalidArgument(format!("invalid address: {}", addr)))
            {
                if let Some(hash_set) = read_blocks.get_blocks_created_by(address) {
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

    let blocks_ids = blocks
        .iter()
        .map(|block| block.header.id)
        .collect::<Vec<BlockId>>();

    let blocks_status = grpc.consensus_controller.get_block_statuses(&blocks_ids);

    let result = blocks
        .iter()
        .zip(blocks_status)
        .map(|(block, block_graph_status)| grpc_model::BlockWrapper {
            block_id: block.header.id.to_string(),
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
    _massa_modelsrequest: tonic::Request<grpc_api::GetNextBlockBestParentsRequest>,
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
    let storage = grpc.storage.clone_without_refs();
    let inner_req: grpc_api::GetOperationsRequest = request.into_inner();

    let mut operations_ids = Vec::new();
    let mut filter_ope_types = Vec::new();

    inner_req.filters.into_iter().for_each(|query| {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::get_operations_filter::Filter::OperationIds(ids) => {
                    let ids = ids
                        .operation_ids
                        .into_iter()
                        .filter_map(|id| match OperationId::from_str(id.as_str()) {
                            Ok(ope) => Some(ope),
                            Err(e) => {
                                warn!("Invalid operation id: {}", e);
                                None
                            }
                        })
                        .collect::<Vec<OperationId>>();

                    operations_ids.extend(ids.iter().copied());
                }
                grpc_api::get_operations_filter::Filter::OperationTypes(ope_types) => {
                    filter_ope_types.extend_from_slice(&ope_types.op_types);
                }
            }
        }
    });

    if operations_ids.is_empty() {
        return Err(GrpcError::InvalidArgument(
            "no operations ids specified".to_string(),
        ));
    }

    if operations_ids.len() as u32 > grpc.grpc_config.max_operation_ids_per_request {
        return Err(GrpcError::InvalidArgument(format!("too many operations received. Only a maximum of {} operations are accepted per request", grpc.grpc_config.max_operation_ids_per_request)));
    }

    let read_blocks = storage.read_blocks();
    let read_ops = storage.read_operations();

    // Get the operations and the list of blocks that contain them from storage
    let storage_info: Vec<(&SecureShareOperation, HashSet<BlockId>)> = operations_ids
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

    let operations: Vec<grpc_model::OperationWrapper> = storage_info
        .into_iter()
        .filter_map(|secure_share| {
            let (secure_share, block_ids) = secure_share;
            let ope_type: grpc_model::OpType = secure_share.content.op.clone().into();
            if !filter_ope_types.is_empty() && !filter_ope_types.contains(&(ope_type as i32)) {
                return None;
            }

            Some(grpc_model::OperationWrapper {
                id: secure_share.id.to_string(),
                thread: secure_share
                    .content_creator_address
                    .get_thread(grpc.grpc_config.thread_count) as u32,
                operation: Some((*secure_share).clone().into()),
                block_ids: block_ids.into_iter().map(|id| id.to_string()).collect(),
            })
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
    let mut event_filter = EventFilter::default();
    for query in request.into_inner().filters {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::sc_execution_events_filter::Filter::SlotRange(slot_range) => {
                    event_filter.start = slot_range.start_slot.map(|slot| slot.into());
                    event_filter.end = slot_range.end_slot.map(|slot| slot.into());
                }
                grpc_api::sc_execution_events_filter::Filter::CallerAddress(caller_address) => {
                    event_filter.original_caller_address =
                        Some(Address::from_str(&caller_address)?);
                }
                grpc_api::sc_execution_events_filter::Filter::EmitterAddress(emitter_address) => {
                    event_filter.emitter_address = Some(Address::from_str(&emitter_address)?);
                }
                grpc_api::sc_execution_events_filter::Filter::OriginalOperationId(operation_id) => {
                    event_filter.original_operation_id =
                        Some(OperationId::from_str(&operation_id)?);
                }
                grpc_api::sc_execution_events_filter::Filter::IsFailure(is_failure) => {
                    event_filter.is_error = Some(is_failure);
                }
                grpc_api::sc_execution_events_filter::Filter::Status(status) => {
                    // See grpc_model::ScExecutionEventStatus
                    match status {
                        1 => event_filter.is_final = Some(true),
                        2 => event_filter.is_final = Some(false),
                        _ => event_filter.is_final = None,
                    }
                }
            }
        }
    }

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

    // checked_div
    let throughput = stats
        .final_executed_operations_count
        .checked_div(nb_sec_range as usize)
        .unwrap_or_default() as u32;

    Ok(grpc_api::GetTransactionsThroughputResponse { throughput })
}

/// Get query state
pub(crate) fn query_state(
    _grpc: &MassaPublicGrpc,
    _request: tonic::Request<grpc_api::QueryStateRequest>,
) -> Result<grpc_api::QueryStateResponse, GrpcError> {
    Err(GrpcError::Unimplemented("query_state".to_string()))
}
