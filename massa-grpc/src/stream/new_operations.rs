// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaPublicGrpc;
use futures_util::StreamExt;
use massa_proto_rs::massa::api::v1::{self as grpc_api, NewOperationsRequest};
use massa_proto_rs::massa::model::v1 as grpc_model;
use std::pin::Pin;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::error;

/// Type declaration for NewOperations
pub type NewOperationsStreamType = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc_api::NewOperationsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

fn get_request_parameters(request: NewOperationsRequest) -> Vec<i32> {
    let mut filter_ope_types = Vec::new();

    request.filters.into_iter().for_each(|query| {
        if let Some(filter) = query.operation_types {
            filter_ope_types.extend_from_slice(&filter.op_types);
        }
    });
    filter_ope_types
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
    let mut subscriber = grpc.pool_channels.operation_sender.subscribe();

    tokio::spawn(async move {
        if let Some(Ok(request)) = in_stream.next().await {
            // Spawn a new task for sending new operations
            let mut filters = get_request_parameters(request);

            loop {
                select! {
                    // Receive a new operation from the subscriber
                     event = subscriber.recv() => {
                        match event {
                            Ok(massa_operation) => {
                                // Check if the operation should be sent
                                if !should_send(&filters, grpc_model::OpType::from(massa_operation.clone().content.op) as i32) {
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
                                    Ok(data) => {
                                        // Update current filter
                                        filters = get_request_parameters(data);
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

fn should_send(filters: &Vec<i32>, ope_type: i32) -> bool {
    filters.is_empty() || filters.contains(&ope_type)
}
