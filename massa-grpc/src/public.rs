// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaPublicGrpc;
use itertools::izip;
use massa_hash::Hash;
use massa_models::address::Address;
use massa_models::block::{Block, BlockGraphStatus};
use massa_models::block_id::BlockId;
use massa_models::execution::EventFilter;
use massa_models::operation::{OperationId, SecureShareOperation};
use massa_models::prehash::PreHashSet;
use massa_models::slot::Slot;
use massa_models::timeslots::{self, get_latest_block_slot_at_timestamp};
use massa_proto_rs::massa::api::v1 as grpc_api;
use massa_proto_rs::massa::model::v1 as grpc_model;
use massa_time::MassaTime;
use std::collections::HashSet;
use std::str::FromStr;
use tracing::log::warn;

/// Get blocks
pub(crate) fn execute_read_only_call(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::ExecuteReadOnlyCallRequest>,
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
                            let mut v = Vec::new();
                            v.push(addr);
                            addresses = Some(v);
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
                        .into_iter()
                        .filter_map(|block_id| {
                            if let Some(block) = read_blocks
                                .get(&block_id)
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
                        return Some((add.clone(), addrs.key));
                    } else {
                        return None;
                    }
                }
                _ => None,
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

// Get node version
pub(crate) fn get_mip_status(
    grpc: &MassaPublicGrpc,
    _request: tonic::Request<grpc_api::GetMipStatusRequest>,
) -> Result<grpc_api::GetMipStatusResponse, GrpcError> {
    let mip_store_status_ = grpc.mip_store.get_mip_status();
    let mip_store_status: Result<Vec<grpc_model::MipStatusEntry>, GrpcError> = mip_store_status_
        .iter()
        .map(|(mip_info, state_id_)| {
            let state_id = grpc_model::ComponentStateId::from(state_id_);
            Ok(grpc_model::MipStatusEntry {
                mip_info: Some(grpc_model::MipInfo::from(mip_info)),
                state_id: i32::from(state_id),
            })
        })
        .collect();

    Ok(grpc_api::GetMipStatusResponse {
        mipstatus_entries: mip_store_status?,
    })
}

/// Get next block best parents
pub(crate) fn get_next_block_best_parents(
    grpc: &MassaPublicGrpc,
    massa_modelsrequest: tonic::Request<grpc_api::GetNextBlockBestParentsRequest>,
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
    // let storage = grpc.storage.clone_without_refs();
    // let inner_req: grpc_api::GetOperationsRequest = request.into_inner();

    // let mut operations_ids = Vec::new();
    // let mut filter_ope_types = Vec::new();

    // inner_req.filters.into_iter().for_each(|query| {
    //     if let Some(filter) = query.filter {
    //         match filter {
    //             grpc_api::get_operations_filter::Filter::OperationIds(ids) => {
    //                 let ids = ids
    //                     .operation_ids
    //                     .into_iter()
    //                     .filter_map(|id| match OperationId::from_str(id.as_str()) {
    //                         Ok(ope) => Some(ope),
    //                         Err(e) => {
    //                             warn!("Invalid operation id: {}", e);
    //                             None
    //                         }
    //                     })
    //                     .collect::<Vec<OperationId>>();

    //                 operations_ids.extend(ids.iter().map(|id| id.clone()));
    //             }
    //             grpc_api::get_operations_filter::Filter::OperationTypes(ope_types) => {
    //                 filter_ope_types.extend_from_slice(&ope_types.op_types);
    //             }
    //         }
    //     }
    // });

    // if operations_ids.is_empty() {
    //     return Err(GrpcError::InvalidArgument(
    //         "no operations ids specified".to_string(),
    //     ));
    // }

    // if operations_ids.len() as u32 > grpc.grpc_config.max_operation_ids_per_request {
    //     return Err(GrpcError::InvalidArgument(format!("too many operations received. Only a maximum of {} operations are accepted per request", grpc.grpc_config.max_operation_ids_per_request)));
    // }

    // let read_blocks = storage.read_blocks();
    // let read_ops = storage.read_operations();

    // // Get the operations and the list of blocks that contain them from storage
    // let storage_info: Vec<(&SecureShareOperation, HashSet<BlockId>)> = operations_ids
    //     .iter()
    //     .filter_map(|ope_id| {
    //         read_ops.get(ope_id).map(|secure_share| {
    //             let block_ids = read_blocks
    //                 .get_blocks_by_operation(ope_id)
    //                 .map(|hashset| hashset.iter().cloned().collect::<HashSet<BlockId>>())
    //                 .unwrap_or_default();

    //             (secure_share, block_ids)
    //         })
    //     })
    //     .collect();

    // // Keep only the ops id (found in storage)
    // let ops: Vec<OperationId> = storage_info.iter().map(|(op, _)| op.id).collect();

    // // Get the speculative and final execution status of the operations
    // let exec_statuses: Vec<_> = grpc
    //     .execution_controller
    //     .get_ops_exec_status(&ops)
    //     .into_iter()
    //     .map(|(spec_exec, final_exec)| match (spec_exec, final_exec) {
    //         (Some(true), Some(true)) => {
    //             vec![
    //                 grpc_model::OperationStatus::Success.into(),
    //                 grpc_model::OperationStatus::Final.into(),
    //             ]
    //         }
    //         (Some(false), Some(false)) => {
    //             vec![
    //                 grpc_model::OperationStatus::Failure.into(),
    //                 grpc_model::OperationStatus::Final.into(),
    //             ]
    //         }
    //         (Some(true), None) => {
    //             vec![
    //                 grpc_model::OperationStatus::Success.into(),
    //                 grpc_model::OperationStatus::Pending.into(),
    //             ]
    //         }
    //         (Some(false), None) => {
    //             vec![
    //                 grpc_model::OperationStatus::Failure.into(),
    //                 grpc_model::OperationStatus::Pending.into(),
    //             ]
    //         }
    //         _ => {
    //             vec![grpc_model::OperationStatus::Unknown.into()]
    //         }
    //     })
    //     .collect();

    // Gather all values into a vector of OperationWrapper instances
    // let operations: Vec<grpc_model::OperationWrapper> = ops
    //     .into_iter()
    //     .zip(storage_info.into_iter())
    //     .zip(exec_statuses.into_iter())
    //     .filter_map(|((id, (operation, in_blocks)), exec_status)| {
    //         // check the operation status with provided filter
    //         // if !filter_ope_types.iter().any(|f| exec_status.contains(f)) {
    //         //     return None;
    //         // }
    //         let ope_type: grpc_model::OpType = operation.content.op.clone().into();
    //         if !filter_ope_types.contains(&(ope_type as i32)) {
    //             return None;
    //         }

    //         Some(grpc_model::OperationWrapper {
    //             id: id.to_string(),
    //             thread: operation
    //                 .content_creator_address
    //                 .get_thread(grpc.grpc_config.thread_count) as u32,
    //             operation: Some((*operation).clone().into()),
    //             block_ids: in_blocks.into_iter().map(|id| id.to_string()).collect(),
    //         })
    //     })
    //     .collect();

    // Ok(grpc_api::GetOperationsResponse {
    //     wrapped_operations: operations,
    // })
    Err(GrpcError::Unimplemented("get_operations".to_string()))
}

/// Get smart contract execution events
pub(crate) fn get_sc_execution_events(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::GetScExecutionEventsRequest>,
) -> Result<grpc_api::GetScExecutionEventsResponse, GrpcError> {
    // let inner_req: grpc_api::GetScExecutionEventsRequest = request.into_inner();
    // let id = inner_req.id;

    // let event_filter = inner_req
    //     .query
    //     .map_or(Ok(EventFilter::default()), |query| {
    //         query.filter.map_or(Ok(EventFilter::default()), |filter| {
    //             filter.try_into().map_err(|e| {
    //                 GrpcError::InvalidArgument(format!("failed to parse filter due to: {}", e))
    //             })
    //         })
    //     })?;

    // // Get the current slot.
    // let now: MassaTime = MassaTime::now()?;
    // let current_slot = get_latest_block_slot_at_timestamp(
    //     grpc.grpc_config.thread_count,
    //     grpc.grpc_config.t0,
    //     grpc.grpc_config.genesis_timestamp,
    //     now,
    // )?
    // .unwrap_or_else(|| Slot::new(0, 0));

    // // Create the context for the response.
    // let context = Some(grpc_api::GetScExecutionEventsContext {
    //     slot: Some(current_slot.into()),
    // });

    // let events: Vec<grpc_model::ScExecutionEvent> = grpc
    //     .execution_controller
    //     .get_filtered_sc_output_event(event_filter)
    //     .into_iter()
    //     .map(|event| event.into())
    //     .collect();

    // Ok(grpc_api::GetScExecutionEventsResponse {
    //     id,
    //     context,
    //     events,
    // })
    Err(GrpcError::Unimplemented(
        "get_sc_execution_events".to_string(),
    ))
}

//  Get selector draws
pub(crate) fn get_selector_draws(
    _grpc: &MassaPublicGrpc,
    _request: tonic::Request<grpc_api::GetSelectorDrawsRequest>,
) -> Result<grpc_api::GetSelectorDrawsResponse, GrpcError> {
    // let inner_req = request.into_inner();
    // let id = inner_req.id;

    // let addresses = inner_req
    //     .queries
    //     .into_iter()
    //     .map(|query| match query.filter {
    //         Some(filter) => Address::from_str(filter.address.as_str()).map_err(|e| e.into()),
    //         None => Err(GrpcError::InvalidArgument("filter is missing".to_string())),
    //     })
    //     .collect::<Result<Vec<_>, _>>()?;

    // // get future draws from selector
    // let selection_draws = {
    //     let cur_slot = match timeslots::get_current_latest_block_slot(
    //         grpc.grpc_config.thread_count,
    //         grpc.grpc_config.t0,
    //         grpc.grpc_config.genesis_timestamp,
    //     ) {
    //         Ok(slot) => slot.unwrap_or_else(Slot::min),
    //         Err(e) => {
    //             warn!("failed to get current slot with error: {}", e);
    //             Slot::min()
    //         }
    //     };

    //     let slot_end = Slot::new(
    //         cur_slot
    //             .period
    //             .saturating_add(grpc.grpc_config.draw_lookahead_period_count),
    //         cur_slot.thread,
    //     );
    //     addresses
    //         .iter()
    //         .map(|addr| {
    //             let (nt_block_draws, nt_endorsement_draws) = grpc
    //                 .selector_controller
    //                 .get_address_selections(addr, cur_slot, slot_end)
    //                 .unwrap_or_default();

    //             let mut proto_nt_block_draws = Vec::with_capacity(addresses.len());
    //             let mut proto_nt_endorsement_draws = Vec::with_capacity(addresses.len());
    //             let iterator = izip!(nt_block_draws.into_iter(), nt_endorsement_draws.into_iter());
    //             for (next_block_draw, next_endorsement_draw) in iterator {
    //                 proto_nt_block_draws.push(next_block_draw.into());
    //                 proto_nt_endorsement_draws.push(next_endorsement_draw.into());
    //             }

    //             (proto_nt_block_draws, proto_nt_endorsement_draws)
    //         })
    //         .collect::<Vec<_>>()
    // };

    // // Compile results
    // let mut res = Vec::with_capacity(addresses.len());
    // let iterator = izip!(addresses.into_iter(), selection_draws.into_iter());
    // for (address, (next_block_draws, next_endorsement_draws)) in iterator {
    //     res.push(grpc_model::SelectorDraws {
    //         address: address.to_string(),
    //         next_block_draws,
    //         next_endorsement_draws,
    //     });
    // }

    // Ok(grpc_api::GetSelectorDrawsResponse {
    //     id,
    //     selector_draws: res,
    // })
    Err(GrpcError::Unimplemented("get_selector_draws".to_string()))
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
