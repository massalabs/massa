// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::config::GrpcConfig;
use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaPublicGrpc;
use crate::SlotRange;
use futures_util::StreamExt;
use massa_models::address::Address;
use massa_models::block::SecureShareBlock;
use massa_models::block_id::BlockId;
use massa_models::slot::Slot;
use massa_proto_rs::massa::api::v1::{self as grpc_api};
use std::collections::HashSet;
use std::io::ErrorKind;
use std::pin::Pin;
use std::str::FromStr;
use tokio::select;
use tonic::{Request, Streaming};
use tracing::{error, warn};

/// Type declaration for NewBlocks
pub type NewBlocksStreamType = Pin<
    Box<
        dyn futures_util::Stream<Item = Result<grpc_api::NewBlocksResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

// Type declaration for NewBlocksFilter
#[derive(Clone, Debug)]
struct Filter {
    // Block ids to filter
    block_ids: Option<HashSet<BlockId>>,
    // Addresses to filter
    addresses: Option<HashSet<Address>>,
    // Slot range to filter
    slot_ranges: Option<HashSet<SlotRange>>,
}

/// Creates a new stream of new produced and received blocks
pub(crate) async fn new_blocks(
    grpc: &MassaPublicGrpc,
    request: Request<Streaming<grpc_api::NewBlocksRequest>>,
) -> Result<NewBlocksStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new blocks channel
    let mut subscriber = grpc.consensus_broadcasts.block_sender.subscribe();
    // Clone grpc to be able to use it in the spawned task
    let grpc_config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        if let Some(Ok(request)) = in_stream.next().await {
            let mut filters = match get_filter(request, &grpc_config) {
                Ok(filter) => filter,
                Err(err) => {
                    error!("failed to get filter: {}", err);
                    // Send the error response back to the client
                    if let Err(e) = tx.send(Err(err.into())).await {
                        error!("failed to send back NewBlocks error response: {}", e);
                    }
                    return;
                }
            };

            loop {
                select! {
                    // Receive a new block from the subscriber
                    event = subscriber.recv() => {
                        match event {
                            Ok(massa_block) => {
                                // Check if the block should be sent
                                if !should_send(&massa_block, &filters, &grpc_config) {
                                    continue;
                                }
                                // Send the new block through the channel
                                if let Err(e) = tx.send(Ok(grpc_api::NewBlocksResponse {
                                    signed_block: Some(massa_block.into())
                                })).await {
                                    error!("failed to send new block : {}", e);
                                    break;
                                }
                            },
                            Err(e) => error!("error on receive new block : {}", e)
                        }
                    },
                    res = in_stream.next() => {
                        match res {
                            Some(res) => {
                                match res {
                                    Ok(message) => {
                                        // Update current filter
                                        filters = match get_filter(message, &grpc_config) {
                                            Ok(filter) => filter,
                                            Err(err) => {
                                                error!("failed to get filter: {}", err);
                                                // Send the error response back to the client
                                                if let Err(e) = tx.send(Err(err.into())).await {
                                                    error!("failed to send back NewBlocks error response: {}", e);
                                                }
                                                return;
                                            }
                                        };
                                    },
                                    Err(err) => {
                                        // Check if the error matches any IO errors
                                        if let Some(io_err) = match_for_io_error(&err) {
                                            if io_err.kind() == ErrorKind::BrokenPipe {
                                                warn!("client disconnected, broken pipe: {}", io_err);
                                                break;
                                            }
                                        }
                                        error!("{}", err);
                                        // Send the error response back to the client
                                        if let Err(e) = tx.send(Err(err)).await {
                                            error!("failed to send back NewBlocks error response: {}", e);
                                            break;
                                        }
                                    }
                            }
                        },
                            None => {
                                // The client has disconnected
                                break;
                            },
                        }
                    }
                }
            }
        } else {
            error!("empty request");
        }
    });

    // Create a new stream from the received channel
    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    // Return the new stream of blocks
    Ok(Box::pin(out_stream) as NewBlocksStreamType)
}

// This function returns a filter from the request
fn get_filter(
    request: grpc_api::NewBlocksRequest,
    grpc_config: &GrpcConfig,
) -> Result<Filter, GrpcError> {
    if request.filters.len() as u32 > grpc_config.max_filters_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many filters received. Only a maximum of {} filters are accepted per request",
            grpc_config.max_filters_per_request
        )));
    }

    let mut block_ids_filter: Option<HashSet<BlockId>> = None;
    let mut addresses_filter: Option<HashSet<Address>> = None;
    let mut slot_ranges_filter: Option<HashSet<SlotRange>> = None;

    // Get params filter from the request.
    for query in request.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::new_blocks_filter::Filter::BlockIds(ids) => {
                    if ids.block_ids.len() as u32 > grpc_config.max_block_ids_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many block ids received. Only a maximum of {} block ids are accepted per request",
                            grpc_config.max_block_ids_per_request
                        )));
                    }

                    let block_ids = block_ids_filter.get_or_insert_with(HashSet::new);
                    for block_id in ids.block_ids {
                        block_ids.insert(BlockId::from_str(&block_id).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid block id: {}", block_id))
                        })?);
                    }
                }
                grpc_api::new_blocks_filter::Filter::Addresses(addrs) => {
                    if addrs.addresses.len() as u32 > grpc_config.max_addresses_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many addresses received. Only a maximum of {} addresses are accepted per request",
                         grpc_config.max_addresses_per_request
                        )));
                    }

                    let addresses = addresses_filter.get_or_insert_with(HashSet::new);
                    for address in addrs.addresses {
                        addresses.insert(Address::from_str(&address).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid address: {}", address))
                        })?);
                    }
                }
                grpc_api::new_blocks_filter::Filter::SlotRange(s_range) => {
                    let slot_ranges = slot_ranges_filter.get_or_insert_with(HashSet::new);
                    if slot_ranges.len() as u32 > grpc_config.max_slot_ranges_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many slot ranges received. Only a maximum of {} slot ranges are accepted per request",
                         grpc_config.max_slot_ranges_per_request
                        )));
                    }

                    let start_slot = s_range.start_slot.map(|s| s.into());
                    let end_slot = s_range.end_slot.map(|s| s.into());

                    let slot_range = SlotRange {
                        start_slot,
                        end_slot,
                    };
                    slot_range.check()?;
                    slot_ranges.insert(slot_range);
                }
            }
        }
    }

    Ok(Filter {
        block_ids: block_ids_filter,
        addresses: addresses_filter,
        slot_ranges: slot_ranges_filter,
    })
}

// This function checks if the block should be sent
fn should_send(
    signed_block: &SecureShareBlock,
    filters: &Filter,
    grpc_config: &GrpcConfig,
) -> bool {
    if let Some(block_ids) = &filters.block_ids {
        if !block_ids.contains(&signed_block.id) {
            return false;
        }
    }

    if let Some(addresses) = &filters.addresses {
        if !addresses.contains(&signed_block.content_creator_address) {
            return false;
        }
    }

    if let Some(slot_ranges) = &filters.slot_ranges {
        let mut start_slot = Slot::new(0, 0); // inclusive
        let mut end_slot = Slot::new(u64::MAX, grpc_config.thread_count - 1); // exclusive

        for slot_range in slot_ranges {
            start_slot = start_slot.max(slot_range.start_slot.unwrap_or_else(|| Slot::new(0, 0)));
            end_slot = end_slot.min(
                slot_range
                    .end_slot
                    .unwrap_or_else(|| Slot::new(u64::MAX, grpc_config.thread_count - 1)),
            );
        }
        end_slot = end_slot.max(start_slot);
        let current_slot = signed_block.content.header.content.slot;

        return current_slot >= start_slot // inclusive
            && current_slot < end_slot; // exclusive
    }

    true
}
