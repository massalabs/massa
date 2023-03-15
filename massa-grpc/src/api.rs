use crate::error::GrpcError;
use crate::handler::MassaGrpcService;
use itertools::izip;
use massa_models::address::Address;
use massa_models::slot::Slot;
use massa_models::timeslots;
use massa_proto::massa::api::v1::{
    self as grpc, BestParentTuple, Block, GetBlocksBySlotRequest, GetBlocksBySlotResponse,
    GetDatastoreEntriesResponse, GetNextBlockBestParentsRequest, GetNextBlockBestParentsResponse,
    GetSelectorDrawsResponse, GetTransactionsThroughputRequest, GetTransactionsThroughputResponse,
    GetVersionResponse,
};
use std::str::FromStr;
use tonic::Request;

/// get version
pub(crate) fn get_version(
    grpc: &MassaGrpcService,
    request: Request<grpc::GetVersionRequest>,
) -> Result<GetVersionResponse, GrpcError> {
    Ok(GetVersionResponse {
        id: request.into_inner().id,
        version: grpc.version.to_string(),
    })
}

/// Get multiple datastore entries.
pub(crate) fn get_datastore_entries(
    grpc: &MassaGrpcService,
    request: Request<grpc::GetDatastoreEntriesRequest>,
) -> Result<GetDatastoreEntriesResponse, GrpcError> {
    let execution_controller = grpc.execution_controller.clone();
    let inner_req = request.into_inner();
    let id = inner_req.id.clone();

    let filters = inner_req
        .queries
        .into_iter()
        .map(|query| {
            let filter = query.filter.unwrap();
            Address::from_str(filter.address.as_str()).map(|address| (address, filter.key))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let entries = execution_controller
        .get_final_and_active_data_entry(filters)
        .into_iter()
        .map(|output| grpc::BytesMapFieldEntry {
            //TODO this behaviour should be confirmed
            key: output.0.unwrap_or_default(),
            value: output.1.unwrap_or_default(),
        })
        .collect();

    Ok(GetDatastoreEntriesResponse { id, entries })
}

pub(crate) fn get_selector_draws(
    grpc: &MassaGrpcService,
    request: Request<grpc::GetSelectorDrawsRequest>,
) -> Result<GetSelectorDrawsResponse, GrpcError> {
    let selector_controller = grpc.selector_controller.clone();
    let config = grpc.grpc_config.clone();
    let inner_req = request.into_inner();
    let id = inner_req.id.clone();

    let addresses = inner_req
        .queries
        .into_iter()
        .map(|query| Address::from_str(query.filter.unwrap().address.as_str()))
        .collect::<Result<Vec<_>, _>>()?;

    // get future draws from selector
    let selection_draws = {
        let cur_slot = timeslots::get_current_latest_block_slot(
            config.thread_count,
            config.t0,
            config.genesis_timestamp,
        )
        .expect("could not get latest current slot")
        .unwrap_or_else(|| Slot::new(0, 0));
        let slot_end = Slot::new(
            cur_slot
                .period
                .saturating_add(config.draw_lookahead_period_count),
            cur_slot.thread,
        );
        addresses
            .iter()
            .map(|addr| {
                let (nt_block_draws, nt_endorsement_draws) = selector_controller
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

    Ok(GetSelectorDrawsResponse {
        id,
        selector_draws: res,
    })
}

/// Get next block best parents
pub(crate) fn get_next_block_best_parents(
    grpc: &MassaGrpcService,
    request: Request<GetNextBlockBestParentsRequest>,
) -> Result<GetNextBlockBestParentsResponse, GrpcError> {
    let inner_req = request.into_inner();
    let parents = grpc
        .consensus_controller
        .get_best_parents()
        .into_iter()
        .map(|p| BestParentTuple {
            block_id: p.0.to_string(),
            period: p.1,
        })
        .collect();
    Ok(GetNextBlockBestParentsResponse {
        id: inner_req.id,
        data: parents,
    })
}

/// get transactions throughput
pub(crate) fn get_transactions_throughput(
    grpc: &MassaGrpcService,
    request: Request<GetTransactionsThroughputRequest>,
) -> Result<GetTransactionsThroughputResponse, GrpcError> {
    let stats = grpc.execution_controller.get_stats();
    let nb_sec_range = stats
        .time_window_end
        .saturating_sub(stats.time_window_start)
        .to_duration()
        .as_secs();

    // checked_div
    let tx_s = stats
        .final_executed_operations_count
        .checked_div(nb_sec_range as usize)
        .unwrap_or_default() as u32;

    Ok(GetTransactionsThroughputResponse {
        id: request.into_inner().id,
        tx_s,
    })
}

/// get blocks by slots
pub(crate) fn get_blocks_by_slots(
    gprc: &MassaGrpcService,
    request: Request<GetBlocksBySlotRequest>,
) -> Result<GetBlocksBySlotResponse, GrpcError> {
    let inner_req = request.into_inner();
    let consensus_controller = gprc.consensus_controller.clone();
    let storage = gprc.storage.clone_without_refs();

    let mut blocks = vec![];

    for slot in inner_req.slots.into_iter() {
        let block_id_option = consensus_controller.get_blockclique_block_at_slot(Slot {
            period: slot.period,
            thread: slot.thread as u8,
        });

        let block_id = match block_id_option {
            Some(id) => id,
            None => continue,
        };
        let res = storage.read_blocks().get(&block_id).map(|b| {
            // todo rework ?
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
                .map(|endorsement| grpc::SecureShareEndorsement {
                    content: Some(grpc::Endorsement {
                        slot: Some(grpc::Slot {
                            period: endorsement.content.slot.period,
                            thread: endorsement.content.slot.thread as u32,
                        }),
                        index: endorsement.content.index,
                        endorsed_block: endorsement.content.endorsed_block.to_string(),
                    }),
                    signature: endorsement.signature.to_string(),
                    content_creator_pub_key: endorsement.content_creator_pub_key.to_string(),
                    content_creator_address: endorsement.content_creator_address.to_string(),
                    id: endorsement.id.to_string(),
                })
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
                .clone()
                .into_iter()
                .map(|ope| ope.to_string())
                .collect();

            (
                grpc::SecureShareBlockHeader {
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
            blocks.push(Block {
                header: Some(block.0),
                operations: block.1,
            });
        }
    }

    Ok(GetBlocksBySlotResponse {
        id: inner_req.id,
        blocks,
    })
}
