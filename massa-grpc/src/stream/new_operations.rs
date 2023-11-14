// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::config::GrpcConfig;
use crate::error::GrpcError;
use crate::server::MassaPublicGrpc;
use futures_util::StreamExt;
use massa_models::address::Address;
use massa_models::operation::{OperationId, SecureShareOperation};
use massa_proto_rs::massa::api::v1::{self as grpc_api, NewOperationsRequest};
use massa_proto_rs::massa::model::v1 as grpc_model;
use std::collections::HashSet;
use std::pin::Pin;
use std::str::FromStr;
use tokio::select;
use tonic::{Request, Streaming};
use tracing::error;

/// Type declaration for NewOperations
pub type NewOperationsStreamType = Pin<
    Box<
        dyn futures_util::Stream<Item = Result<grpc_api::NewOperationsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

// Type declaration for NewOperationsFilter
#[derive(Debug)]
struct Filter {
    // Operation ids to filter
    operation_ids: Option<HashSet<OperationId>>,
    // Addresses to filter
    addresses: Option<HashSet<Address>>,
    // Operation types to filter
    operation_types: Option<HashSet<i32>>,
}

/// Creates a new stream of new produced and received operations
pub(crate) async fn new_operations(
    grpc: &MassaPublicGrpc,
    request: Request<Streaming<grpc_api::NewOperationsRequest>>,
) -> Result<NewOperationsStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new operations channel
    let mut subscriber = grpc.pool_broadcasts.operation_sender.subscribe();
    // Clone grpc to be able to use it in the spawned task
    // let grpc = grpc.clone();

    let config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        if let Some(Ok(request)) = in_stream.next().await {
            // Spawn a new task for sending new operations
            let mut filters = match get_filter(request, &config) {
                Ok(filter) => filter,
                Err(err) => {
                    error!("failed to get filter: {}", err);
                    // Send the error response back to the client
                    if let Err(e) = tx.send(Err(err.into())).await {
                        error!("failed to send back NewOperations error response: {}", e);
                    }
                    return;
                }
            };

            loop {
                select! {
                    // Receive a new operation from the subscriber
                     event = subscriber.recv() => {
                        match event {
                            Ok(massa_operation) => {
                                // Check if the operation should be sent
                                if !should_send(&massa_operation, &filters) {
                                    continue;
                                }

                                // Send the new operation through the channel
                                if let Err(e) = tx.send(Ok(grpc_api::NewOperationsResponse {signed_operation: Some(massa_operation.into())})).await {
                                    error!("failed to send operation : {}", e);
                                    break;
                                }
                            },
                            Err(e) => error!("{}", e)
                        }
                    },
                    // Receive a new message from the in_stream
                    res = in_stream.next() => {
                        match res {
                            Some(res) => {
                                match res {
                                    Ok(message) => {
                                        // Update current filter
                                        filters = match get_filter(message, &config) {
                                            Ok(filter) => filter,
                                            Err(err) => {
                                                error!("failed to get filter: {}", err);
                                                // Send the error response back to the client
                                                if let Err(e) = tx.send(Err(err.into())).await {
                                                    error!("failed to send back NewOperations error response: {}", e);
                                                }
                                                return;
                                            }
                                        };
                                    },
                                    Err(e) => {
                                        error!("{}", e);
                                        break;
                                    }
                                }
                            },
                            None => {
                                // Client disconnected
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

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as NewOperationsStreamType)
}

// This function returns a filter from the request
fn get_filter(
    request: NewOperationsRequest,
    grpc_config: &GrpcConfig,
) -> Result<Filter, GrpcError> {
    if request.filters.len() as u32 > grpc_config.max_filters_per_request {
        return Err(GrpcError::InvalidArgument(format!(
            "too many filters received. Only a maximum of {} filters are accepted per request",
            grpc_config.max_filters_per_request
        )));
    }

    let mut operation_ids_filter: Option<HashSet<OperationId>> = None;
    let mut addresses_filter: Option<HashSet<Address>> = None;
    let mut operation_types_filter: Option<HashSet<i32>> = None;

    // Get params filter from the request.
    for query in request.filters.into_iter() {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::new_operations_filter::Filter::OperationIds(ids) => {
                    if ids.operation_ids.len() as u32 > grpc_config.max_operation_ids_per_request {
                        return Err(GrpcError::InvalidArgument(format!(
                            "too many operation ids received. Only a maximum of {} operation ids are accepted per request",
                         grpc_config.max_block_ids_per_request
                        )));
                    }
                    let operation_ids = operation_ids_filter.get_or_insert_with(HashSet::new);
                    for id in ids.operation_ids {
                        operation_ids.insert(OperationId::from_str(&id).map_err(|_| {
                            GrpcError::InvalidArgument(format!("invalid operation id: {}", id))
                        })?);
                    }
                }
                grpc_api::new_operations_filter::Filter::Addresses(addrs) => {
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
                grpc_api::new_operations_filter::Filter::OperationTypes(ope_types) => {
                    // The length limited to the number of operation types in the enum
                    if ope_types.op_types.len() as u64 > 6 {
                        return Err(GrpcError::InvalidArgument(
                            "too many operation types received. Only a maximum of 6 operation types are accepted per request".to_string()
                        ));
                    }
                    let operation_types = operation_types_filter.get_or_insert_with(HashSet::new);
                    operation_types.extend(&ope_types.op_types);
                }
            }
        }
    }

    Ok(Filter {
        operation_ids: operation_ids_filter,
        addresses: addresses_filter,
        operation_types: operation_types_filter,
    })
}

// This function checks if the operation should be sent
fn should_send(signed_operation: &SecureShareOperation, filters: &Filter) -> bool {
    if let Some(operation_ids) = &filters.operation_ids {
        if !operation_ids.contains(&signed_operation.id) {
            return false;
        }
    }

    if let Some(addresses) = &filters.addresses {
        if !addresses.contains(&signed_operation.content_creator_address) {
            return false;
        }
    }

    if let Some(operation_types) = &filters.operation_types {
        let op_type = grpc_model::OpType::from(signed_operation.content.op.clone()) as i32;
        if !operation_types.contains(&op_type) {
            return false;
        }
    }

    true
}
