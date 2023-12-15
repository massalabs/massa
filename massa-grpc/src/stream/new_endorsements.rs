// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::config::GrpcConfig;
use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaPublicGrpc;
use futures_util::StreamExt;
use massa_models::address::Address;
use massa_models::block_id::BlockId;
use massa_models::endorsement::{EndorsementId, SecureShareEndorsement};
use massa_proto_rs::massa::api::v1::{self as grpc_api, NewEndorsementsRequest};
use std::collections::HashSet;
use std::io::ErrorKind;
use std::pin::Pin;
use std::str::FromStr;
use tokio::select;
use tonic::{Request, Streaming};
use tracing::{error, warn};

/// Type declaration for NewEndorsements
pub type NewEndorsementsStreamType = Pin<
    Box<
        dyn futures_util::Stream<Item = Result<grpc_api::NewEndorsementsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

// Type declaration for NewEndorsementsFilter
#[derive(Debug)]
struct Filter {
    // Endorsement ids to filter
    endorsement_ids: Option<HashSet<EndorsementId>>,
    // Addresses to filter
    addresses: Option<HashSet<Address>>,
    // Block ids to filter
    block_ids: Option<HashSet<BlockId>>,
}

/// Creates a new stream of new produced and received endorsements
pub(crate) async fn new_endorsements(
    grpc: &MassaPublicGrpc,
    request: Request<Streaming<grpc_api::NewEndorsementsRequest>>,
) -> Result<NewEndorsementsStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new endorsements channel
    let mut subscriber = grpc.pool_broadcasts.endorsement_sender.subscribe();
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
                        error!("failed to send back new_operations error response: {}", e);
                    }
                    return;
                }
            };

            loop {
                select! {
                    // Receive a new endorsement from the subscriber
                    event = subscriber.recv() => {
                        match event {
                            Ok(massa_endorsement) => {
                                // Check if the endorsement should be sent
                                if !should_send(&massa_endorsement, &filters) {
                                    continue;
                                }

                                // Send the new endorsement through the channel
                                if let Err(e) = tx.send(Ok(grpc_api::NewEndorsementsResponse {
                                        signed_endorsement: Some(massa_endorsement.into())
                                })).await {
                                    error!("failed to send new endorsement : {}", e);
                                    break;
                                }
                            },
                            Err(e) => error!("error on receive new endorsement : {}", e)
                        }
                    },
                    // Receive a new message from the in_stream
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
                                                    error!("failed to send back NewEndorsements error response: {}", e);
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
                                            error!("failed to send back NewEndorsements error response: {}", e);
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

    // Return the new stream of endorsements
    Ok(Box::pin(out_stream) as NewEndorsementsStreamType)
}

// This function returns a filter from the request
fn get_filter(
    request: NewEndorsementsRequest,
    grpc_config: &GrpcConfig,
) -> Result<Filter, GrpcError> {
    if request.filters.len() as u32 > grpc_config.max_filters_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many filters received. Only a maximum of {} filters are accepted per request",
            grpc_config.max_filters_per_request
        )));
    }

    let mut endorsement_ids_filter: Option<HashSet<EndorsementId>> = None;
    let mut addresses_filter: Option<HashSet<Address>> = None;
    let mut block_ids_filter: Option<HashSet<BlockId>> = None;

    // Get params filter from the request.
    for query in request.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::new_endorsements_filter::Filter::EndorsementIds(ids) => {
                    if ids.endorsement_ids.len() as u32
                        > grpc_config.max_endorsement_ids_per_request
                    {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many endorsement ids received. Only a maximum of {} endorsement ids are accepted per request",
                         grpc_config.max_block_ids_per_request
                        )));
                    }
                    let endorsement_ids = endorsement_ids_filter.get_or_insert_with(HashSet::new);
                    for id in ids.endorsement_ids {
                        endorsement_ids.insert(EndorsementId::from_str(&id).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid endorsement id: {}", id))
                        })?);
                    }
                }
                grpc_api::new_endorsements_filter::Filter::Addresses(addrs) => {
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
                grpc_api::new_endorsements_filter::Filter::BlockIds(ids) => {
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
            }
        }
    }

    Ok(Filter {
        endorsement_ids: endorsement_ids_filter,
        addresses: addresses_filter,
        block_ids: block_ids_filter,
    })
}

// This function checks if the endorsement should be sent
fn should_send(signed_endorsement: &SecureShareEndorsement, filters: &Filter) -> bool {
    if let Some(endorsement_ids) = &filters.endorsement_ids {
        if !endorsement_ids.contains(&signed_endorsement.id) {
            return false;
        }
    }

    if let Some(addresses) = &filters.addresses {
        if !addresses.contains(&signed_endorsement.content_creator_address) {
            return false;
        }
    }

    if let Some(block_ids) = &filters.block_ids {
        if !block_ids.contains(&signed_endorsement.content.endorsed_block) {
            return false;
        }
    }

    true
}
