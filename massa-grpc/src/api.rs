// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaGrpc;
use itertools::izip;
use massa_models::address::Address;
use massa_models::block::BlockGraphStatus;
use massa_models::block_id::BlockId;
use massa_models::execution::EventFilter;
use massa_models::operation::{OperationId, SecureShareOperation};
use massa_models::prehash::PreHashSet;
use massa_models::slot::Slot;
use massa_models::timeslots::{self, get_latest_block_slot_at_timestamp};
use massa_proto_rs::massa::api::v1 as grpc_api;
use massa_proto_rs::massa::model::v1 as grpc_model;
use massa_time::MassaTime;
use std::str::FromStr;
use tracing::log::warn;

/// Default offset
const DEFAULT_OFFSET: u64 = 1;
/// Default limit
const DEFAULT_LIMIT: u64 = 50;

/// Get blocks
pub(crate) fn get_blocks(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc_api::GetBlocksRequest>,
) -> Result<grpc_api::GetBlocksResponse, GrpcError> {
    let inner_req = request.into_inner();

    // Get the block IDs from the request.
    let blocks_ids: Vec<BlockId> = inner_req
        .queries
        .into_iter()
        .take(grpc.grpc_config.max_block_ids_per_request as usize + 1)
        .map(|query| {
            // Get the block ID from the query.
            query
                .filter
                .ok_or_else(|| GrpcError::InvalidArgument("filter is missing".to_string()))
                .and_then(|filter| {
                    BlockId::from_str(filter.id.as_str()).map_err(|_| {
                        GrpcError::InvalidArgument(format!("invalid block id: {}", filter.id))
                    })
                })
        })
        .collect::<Result<_, _>>()?;

    if blocks_ids.len() as u32 > grpc.grpc_config.max_block_ids_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many block ids received. Only a maximum of {} block ids are accepted per request",
            grpc.grpc_config.max_block_ids_per_request
        )));
    }

    // Get the current slot.
    let now: MassaTime = MassaTime::now()?;
    let current_slot = get_latest_block_slot_at_timestamp(
        grpc.grpc_config.thread_count,
        grpc.grpc_config.t0,
        grpc.grpc_config.genesis_timestamp,
        now,
    )?
    .unwrap_or_else(|| Slot::new(0, 0));

    // Create the context for the response.
    let context = Some(grpc_api::BlocksContext {
        slot: Some(current_slot.into()),
    });

    let storage = grpc.storage.clone_without_refs();
    let blocks = blocks_ids
        .into_iter()
        .filter_map(|id| {
            let content = if let Some(wrapped_block) = storage.read_blocks().get(&id) {
                wrapped_block.content.clone()
            } else {
                return None;
            };

            if let Some(graph_status) = grpc
                .consensus_controller
                .get_block_statuses(&[id])
                .into_iter()
                .next()
            {
                let mut status = Vec::new();

                if graph_status == BlockGraphStatus::Final {
                    status.push(grpc_model::BlockStatus::Final.into());
                };
                if graph_status == BlockGraphStatus::ActiveInBlockclique {
                    status.push(grpc_model::BlockStatus::InBlockclique.into());
                };
                if graph_status == BlockGraphStatus::ActiveInBlockclique
                    || graph_status == BlockGraphStatus::ActiveInAlternativeCliques
                {
                    status.push(grpc_model::BlockStatus::Candidate.into());
                };
                if graph_status == BlockGraphStatus::Discarded {
                    status.push(grpc_model::BlockStatus::Discarded.into());
                };

                return Some(grpc_model::BlockWrapper {
                    id: id.to_string(),
                    block: Some(content.into()),
                    status,
                });
            }

            None
        })
        .collect::<Vec<grpc_model::BlockWrapper>>();

    Ok(grpc_api::GetBlocksResponse {
        id: inner_req.id,
        context,
        blocks,
    })
}

/// get blocks by slots
pub(crate) fn get_blocks_by_slots(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc_api::GetBlocksBySlotsRequest>,
) -> Result<grpc_api::GetBlocksBySlotsResponse, GrpcError> {
    let inner_req = request.into_inner();
    let storage = grpc.storage.clone_without_refs();

    let mut blocks = Vec::new();

    for slot in inner_req.slots.into_iter() {
        let Some(block_id) = grpc.consensus_controller.get_blockclique_block_at_slot(Slot {
            period: slot.period,
            thread: slot.thread as u8,
        }) else {
            continue;
        };

        let res = storage.read_blocks().get(&block_id).map(|b| {
            let massa_header = b.clone().content.header;
            let operations: Vec<String> = b
                .content
                .operations
                .iter()
                .map(|ope| ope.to_string())
                .collect();

            (massa_header.into(), operations)
        });

        if let Some(block) = res {
            blocks.push(grpc_model::Block {
                header: Some(block.0),
                operations: block.1,
            });
        }
    }

    Ok(grpc_api::GetBlocksBySlotsResponse {
        id: inner_req.id,
        blocks,
    })
}

/// Get multiple datastore entries
pub(crate) fn get_datastore_entries(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc_api::GetDatastoreEntriesRequest>,
) -> Result<grpc_api::GetDatastoreEntriesResponse, GrpcError> {
    let inner_req = request.into_inner();
    let id = inner_req.id;

    let filters = inner_req
        .queries
        .into_iter()
        .map(|query| match query.filter {
            Some(filter) => Address::from_str(filter.address.as_str())
                .map(|address| (address, filter.key))
                .map_err(|e| e.into()),
            None => Err(GrpcError::InvalidArgument("filter is missing".to_string())),
        })
        .collect::<Result<Vec<_>, _>>()?;

    let entries = grpc
        .execution_controller
        .get_final_and_active_data_entry(filters)
        .into_iter()
        .map(|output| grpc_api::DatastoreEntry {
            final_value: output.0.unwrap_or_default(),
            candidate_value: output.1.unwrap_or_default(),
        })
        .collect();

    Ok(grpc_api::GetDatastoreEntriesResponse { id, entries })
}

/// Get the largest stakers
pub(crate) fn get_largest_stakers(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc_api::GetLargestStakersRequest>,
) -> Result<grpc_api::GetLargestStakersResponse, GrpcError> {
    let inner_req = request.into_inner();
    let id = inner_req.id;

    // Parse the query parameters, if provided.
    let query_res: Result<(u64, u64, Option<grpc_api::LargestStakersFilter>), GrpcError> =
        inner_req
            .query
            .map_or(Ok((DEFAULT_OFFSET, DEFAULT_LIMIT, None)), |query| {
                let limit = if query.limit == 0 {
                    DEFAULT_LIMIT
                } else {
                    query.limit
                };
                let filter = query.filter;
                // If the filter is provided, validate the minimum and maximum roll counts.
                let filter_opt = filter
                    .map(|filter| {
                        if let Some(min_rolls) = filter.min_rolls {
                            if min_rolls == 0 {
                                return Err(GrpcError::InvalidArgument(
                                    "min_rolls should be a positive number".into(),
                                ));
                            }
                            if let Some(max_rolls) = filter.max_rolls {
                                if max_rolls == 0 {
                                    return Err(GrpcError::InvalidArgument(
                                        "max_rolls should be a positive number".into(),
                                    ));
                                }
                                if min_rolls > max_rolls {
                                    return Err(GrpcError::InvalidArgument(format!(
                                        "min_rolls {} cannot be greater than max_rolls {}",
                                        min_rolls, max_rolls
                                    )));
                                }
                            }
                        }

                        Ok(filter)
                    })
                    .transpose()?; // Convert `Option<Result>` to `Result<Option>`.

                Ok((query.offset, limit, filter_opt))
            });

    let (offset, limit, filter_opt) = query_res?;

    // Get the current cycle and slot.
    let now: MassaTime = MassaTime::now()?;

    let latest_block_slot_at_timestamp_result = get_latest_block_slot_at_timestamp(
        grpc.grpc_config.thread_count,
        grpc.grpc_config.t0,
        grpc.grpc_config.genesis_timestamp,
        now,
    );

    let (cur_cycle, cur_slot) = match latest_block_slot_at_timestamp_result {
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

    // Create the context for the response.
    let context = Some(grpc_api::LargestStakersContext {
        slot: Some(cur_slot.into()),
        // IMPORTANT TODO: tmp value because testnet_24 and massa proto latest are not synced atm
        in_downtime: false,
    });

    // Get the list of stakers, filtered by the specified minimum and maximum roll counts.
    let mut staker_vec = grpc
        .execution_controller
        .get_cycle_active_rolls(cur_cycle)
        .into_iter()
        .filter(|(_, rolls)| {
            filter_opt.as_ref().map_or(true, |filter| {
                if let Some(min_rolls) = filter.min_rolls {
                    if *rolls < min_rolls {
                        return false;
                    }
                }
                if let Some(max_rolls) = filter.max_rolls {
                    if *rolls > max_rolls {
                        return false;
                    }
                }
                true
            })
        })
        .map(|(address, roll_counts)| (address.to_string(), roll_counts))
        .collect::<Vec<(String, u64)>>();

    // Sort the stakers by their roll counts in descending order.
    staker_vec.sort_by_key(|&(_, roll_counts)| std::cmp::Reverse(roll_counts));

    // Paginate the stakers based on the specified offset and limit.
    let stakers = staker_vec
        .into_iter()
        .map(|(address, rolls)| grpc_api::LargestStakerEntry { address, rolls })
        .skip(offset as usize)
        .take(limit as usize)
        .collect();

    // Return a response with the given id, context, and the collected stakers.
    Ok(grpc_api::GetLargestStakersResponse {
        id,
        context,
        stakers,
    })
}

// Get node version
pub(crate) fn get_mip_status(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc_api::GetMipStatusRequest>,
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
        id: request.into_inner().id,
        entries: mip_store_status?,
    })
}

/// Get next block best parents
pub(crate) fn get_next_block_best_parents(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc_api::GetNextBlockBestParentsRequest>,
) -> Result<grpc_api::GetNextBlockBestParentsResponse, GrpcError> {
    let inner_req = request.into_inner();
    let parents = grpc
        .consensus_controller
        .get_best_parents()
        .into_iter()
        .map(|p| grpc_api::BlockParent {
            block_id: p.0.to_string(),
            period: p.1,
        })
        .collect();
    Ok(grpc_api::GetNextBlockBestParentsResponse {
        id: inner_req.id,
        parents,
    })
}

/// Get operations
pub(crate) fn get_operations(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc_api::GetOperationsRequest>,
) -> Result<grpc_api::GetOperationsResponse, GrpcError> {
    let storage = grpc.storage.clone_without_refs();
    let inner_req: grpc_api::GetOperationsRequest = request.into_inner();
    let id = inner_req.id;

    let operations_ids: Vec<OperationId> = inner_req
        .queries
        .into_iter()
        .take(grpc.grpc_config.max_operation_ids_per_request as usize + 1)
        .map(|query| {
            query
                .filter
                .ok_or_else(|| GrpcError::InvalidArgument("filter is missing".to_string()))
                .and_then(|filter| {
                    OperationId::from_str(filter.id.as_str()).map_err(|_| {
                        GrpcError::InvalidArgument(format!("invalid operation id: {}", filter.id))
                    })
                })
        })
        .collect::<Result<_, _>>()?;

    if operations_ids.len() as u32 > grpc.grpc_config.max_operation_ids_per_request {
        return Err(GrpcError::InvalidArgument(format!("too many operations received. Only a maximum of {} operations are accepted per request", grpc.grpc_config.max_operation_ids_per_request)));
    }

    // Get the current slot.
    let now: MassaTime = MassaTime::now()?;
    let current_slot = get_latest_block_slot_at_timestamp(
        grpc.grpc_config.thread_count,
        grpc.grpc_config.t0,
        grpc.grpc_config.genesis_timestamp,
        now,
    )?
    .unwrap_or_else(|| Slot::new(0, 0));

    // Create the context for the response.
    let context = Some(grpc_api::OperationsContext {
        slot: Some(current_slot.into()),
    });

    // Get the operations and the list of blocks that contain them from storage
    let storage_info: Vec<(SecureShareOperation, PreHashSet<BlockId>)> = {
        let read_blocks = storage.read_blocks();
        let read_ops = storage.read_operations();
        operations_ids
            .iter()
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

    // Keep only the ops id (found in storage)
    let ops: Vec<OperationId> = storage_info.iter().map(|(op, _)| op.id).collect();

    // Get the speculative and final execution status of the operations
    let exec_statuses: Vec<_> = grpc
        .execution_controller
        .get_ops_exec_status(&ops)
        .into_iter()
        .map(|(spec_exec, final_exec)| match (spec_exec, final_exec) {
            (Some(true), Some(true)) => {
                vec![
                    grpc_model::OperationStatus::Success.into(),
                    grpc_model::OperationStatus::Final.into(),
                ]
            }
            (Some(false), Some(false)) => {
                vec![
                    grpc_model::OperationStatus::Failure.into(),
                    grpc_model::OperationStatus::Final.into(),
                ]
            }
            (Some(true), None) => {
                vec![
                    grpc_model::OperationStatus::Success.into(),
                    grpc_model::OperationStatus::Pending.into(),
                ]
            }
            (Some(false), None) => {
                vec![
                    grpc_model::OperationStatus::Failure.into(),
                    grpc_model::OperationStatus::Pending.into(),
                ]
            }
            _ => {
                vec![grpc_model::OperationStatus::Unknown.into()]
            }
        })
        .collect();

    // Gather all values into a vector of OperationWrapper instances
    let mut operations: Vec<grpc_model::OperationWrapper> = Vec::with_capacity(ops.len());
    let zipped_iterator = izip!(
        ops.into_iter(),
        storage_info.into_iter(),
        exec_statuses.into_iter(),
    );
    for (id, (operation, in_blocks), exec_status) in zipped_iterator {
        operations.push(grpc_model::OperationWrapper {
            id: id.to_string(),
            thread: operation
                .content_creator_address
                .get_thread(grpc.grpc_config.thread_count) as u32,
            operation: Some(operation.into()),
            block_ids: in_blocks.into_iter().map(|id| id.to_string()).collect(),
            status: exec_status,
        });
    }

    Ok(grpc_api::GetOperationsResponse {
        id,
        context,
        operations,
    })
}

/// Get smart contract execution events
pub(crate) fn get_sc_execution_events(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc_api::GetScExecutionEventsRequest>,
) -> Result<grpc_api::GetScExecutionEventsResponse, GrpcError> {
    let inner_req: grpc_api::GetScExecutionEventsRequest = request.into_inner();
    let id = inner_req.id;

    let event_filter = inner_req
        .query
        .map_or(Ok(EventFilter::default()), |query| {
            query.filter.map_or(Ok(EventFilter::default()), |filter| {
                filter.try_into().map_err(|e| {
                    GrpcError::InvalidArgument(format!("failed to parse filter due to: {}", e))
                })
            })
        })?;

    // Get the current slot.
    let now: MassaTime = MassaTime::now()?;
    let current_slot = get_latest_block_slot_at_timestamp(
        grpc.grpc_config.thread_count,
        grpc.grpc_config.t0,
        grpc.grpc_config.genesis_timestamp,
        now,
    )?
    .unwrap_or_else(|| Slot::new(0, 0));

    // Create the context for the response.
    let context = Some(grpc_api::GetScExecutionEventsContext {
        slot: Some(current_slot.into()),
    });

    let events: Vec<grpc_model::ScExecutionEvent> = grpc
        .execution_controller
        .get_filtered_sc_output_event(event_filter)
        .into_iter()
        .map(|event| event.into())
        .collect();

    Ok(grpc_api::GetScExecutionEventsResponse {
        id,
        context,
        events,
    })
}

//  Get selector draws
pub(crate) fn get_selector_draws(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc_api::GetSelectorDrawsRequest>,
) -> Result<grpc_api::GetSelectorDrawsResponse, GrpcError> {
    let inner_req = request.into_inner();
    let id = inner_req.id;

    let addresses = inner_req
        .queries
        .into_iter()
        .map(|query| match query.filter {
            Some(filter) => Address::from_str(filter.address.as_str()).map_err(|e| e.into()),
            None => Err(GrpcError::InvalidArgument("filter is missing".to_string())),
        })
        .collect::<Result<Vec<_>, _>>()?;

    // get future draws from selector
    let selection_draws = {
        let cur_slot = match timeslots::get_current_latest_block_slot(
            grpc.grpc_config.thread_count,
            grpc.grpc_config.t0,
            grpc.grpc_config.genesis_timestamp,
        ) {
            Ok(slot) => slot.unwrap_or_else(Slot::min),
            Err(e) => {
                warn!("failed to get current slot with error: {}", e);
                Slot::min()
            }
        };

        let slot_end = Slot::new(
            cur_slot
                .period
                .saturating_add(grpc.grpc_config.draw_lookahead_period_count),
            cur_slot.thread,
        );
        addresses
            .iter()
            .map(|addr| {
                let (nt_block_draws, nt_endorsement_draws) = grpc
                    .selector_controller
                    .get_address_selections(addr, cur_slot, slot_end)
                    .unwrap_or_default();

                let mut proto_nt_block_draws = Vec::with_capacity(addresses.len());
                let mut proto_nt_endorsement_draws = Vec::with_capacity(addresses.len());
                let iterator = izip!(nt_block_draws.into_iter(), nt_endorsement_draws.into_iter());
                for (next_block_draw, next_endorsement_draw) in iterator {
                    proto_nt_block_draws.push(next_block_draw.into());
                    proto_nt_endorsement_draws.push(next_endorsement_draw.into());
                }

                (proto_nt_block_draws, proto_nt_endorsement_draws)
            })
            .collect::<Vec<_>>()
    };

    // Compile results
    let mut res = Vec::with_capacity(addresses.len());
    let iterator = izip!(addresses.into_iter(), selection_draws.into_iter());
    for (address, (next_block_draws, next_endorsement_draws)) in iterator {
        res.push(grpc_model::SelectorDraws {
            address: address.to_string(),
            next_block_draws,
            next_endorsement_draws,
        });
    }

    Ok(grpc_api::GetSelectorDrawsResponse {
        id,
        selector_draws: res,
    })
}

/// Get transactions throughput
pub(crate) fn get_transactions_throughput(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc_api::GetTransactionsThroughputRequest>,
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

    Ok(grpc_api::GetTransactionsThroughputResponse {
        id: request.into_inner().id,
        throughput,
    })
}

// Get node version
pub(crate) fn get_version(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc_api::GetVersionRequest>,
) -> Result<grpc_api::GetVersionResponse, GrpcError> {
    Ok(grpc_api::GetVersionResponse {
        id: request.into_inner().id,
        version: grpc.version.to_string(),
    })
}
