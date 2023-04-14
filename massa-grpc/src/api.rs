// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaGrpc;
use itertools::izip;
use massa_models::address::Address;
use massa_models::slot::Slot;
use massa_models::timeslots::{self, get_latest_block_slot_at_timestamp};
use massa_proto::massa::api::v1::{self as grpc};
use massa_time::MassaTime;
use std::str::FromStr;
use tracing::log::warn;

/// default offset
const DEFAULT_OFFSET: u64 = 1;
/// default limit
const DEFAULT_LIMIT: u64 = 50;

/// get blocks by slots
pub(crate) fn get_blocks_by_slots(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc::GetBlocksBySlotsRequest>,
) -> Result<grpc::GetBlocksBySlotsResponse, GrpcError> {
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
            // TODO rework ?
            let header = b.clone().content.header;
            // transform to grpc struct
            let parents = header
                .content
                .parents
                .into_iter()
                .map(|p| p.to_string())
                .collect();

            let endorsements = header
                .content
                .endorsements
                .into_iter()
                .map(|endorsement| endorsement.into())
                .collect();

            let block_header = grpc::BlockHeader {
                slot: Some(grpc::Slot {
                    period: header.content.slot.period,
                    thread: header.content.slot.thread as u32,
                }),
                parents,
                operation_merkle_root: header.content.operation_merkle_root.to_string(),
                endorsements,
            };

            let operations: Vec<String> = b
                .content
                .operations
                .iter()
                .map(|ope| ope.to_string())
                .collect();

            (
                grpc::SignedBlockHeader {
                    content: Some(block_header),
                    signature: header.signature.to_string(),
                    content_creator_pub_key: header.content_creator_pub_key.to_string(),
                    content_creator_address: header.content_creator_address.to_string(),
                    id: header.id.to_string(),
                },
                operations,
            )
        });

        if let Some(block) = res {
            blocks.push(grpc::Block {
                header: Some(block.0),
                operations: block.1,
            });
        }
    }

    Ok(grpc::GetBlocksBySlotsResponse {
        id: inner_req.id,
        blocks,
    })
}

/// get multiple datastore entries
pub(crate) fn get_datastore_entries(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc::GetDatastoreEntriesRequest>,
) -> Result<grpc::GetDatastoreEntriesResponse, GrpcError> {
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
        .map(|output| grpc::DatastoreEntry {
            final_value: output.0.unwrap_or_default(),
            candidate_value: output.1.unwrap_or_default(),
        })
        .collect();

    Ok(grpc::GetDatastoreEntriesResponse { id, entries })
}

/// Get the largest stakers.
pub(crate) fn get_largest_stakers(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc::GetLargestStakersRequest>,
) -> Result<grpc::GetLargestStakersResponse, GrpcError> {
    let inner_req = request.into_inner();
    let id = inner_req.id;

    // Parse the query parameters, if provided.
    let query_res: Result<(u64, u64, Option<grpc::LargestStakersFilter>), GrpcError> = inner_req
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
    let current_slot = get_latest_block_slot_at_timestamp(
        grpc.grpc_config.thread_count,
        grpc.grpc_config.t0,
        grpc.grpc_config.genesis_timestamp,
        now,
    )?
    .unwrap_or_else(|| Slot::new(0, 0));
    let current_cycle = current_slot.get_cycle(grpc.grpc_config.periods_per_cycle);

    // Create the context for the response.
    let context = Some(grpc::LargestStakersContext {
        cycle: current_cycle,
        slot: Some(current_slot.into()),
    });

    // Get the list of stakers, filtered by the specified minimum and maximum roll counts.
    let mut staker_vec = grpc
        .execution_controller
        .get_cycle_active_rolls(current_cycle)
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
        .map(|(address, rolls)| grpc::LargestStakerEntry { address, rolls })
        .skip(offset as usize)
        .take(limit as usize)
        .collect();

    // Return a response with the given id, context, and the collected stakers.
    Ok(grpc::GetLargestStakersResponse {
        id,
        context,
        stakers,
    })
}

/// get next block best parents
pub(crate) fn get_next_block_best_parents(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc::GetNextBlockBestParentsRequest>,
) -> Result<grpc::GetNextBlockBestParentsResponse, GrpcError> {
    let inner_req = request.into_inner();
    let parents = grpc
        .consensus_controller
        .get_best_parents()
        .into_iter()
        .map(|p| grpc::BlockParent {
            block_id: p.0.to_string(),
            period: p.1,
        })
        .collect();
    Ok(grpc::GetNextBlockBestParentsResponse {
        id: inner_req.id,
        parents,
    })
}

pub(crate) fn get_selector_draws(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc::GetSelectorDrawsRequest>,
) -> Result<grpc::GetSelectorDrawsResponse, GrpcError> {
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

    // compile results
    let mut res = Vec::with_capacity(addresses.len());
    let iterator = izip!(addresses.into_iter(), selection_draws.into_iter());
    for (address, (next_block_draws, next_endorsement_draws)) in iterator {
        res.push(grpc::SelectorDraws {
            address: address.to_string(),
            next_block_draws,
            next_endorsement_draws,
        });
    }

    Ok(grpc::GetSelectorDrawsResponse {
        id,
        selector_draws: res,
    })
}

/// get transactions throughput
pub(crate) fn get_transactions_throughput(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc::GetTransactionsThroughputRequest>,
) -> Result<grpc::GetTransactionsThroughputResponse, GrpcError> {
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

    Ok(grpc::GetTransactionsThroughputResponse {
        id: request.into_inner().id,
        throughput,
    })
}

// get node version
pub(crate) fn get_version(
    grpc: &MassaGrpc,
    request: tonic::Request<grpc::GetVersionRequest>,
) -> Result<grpc::GetVersionResponse, GrpcError> {
    Ok(grpc::GetVersionResponse {
        id: request.into_inner().id,
        version: grpc.version.to_string(),
    })
}
