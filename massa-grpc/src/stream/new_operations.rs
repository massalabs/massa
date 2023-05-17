// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaGrpc;
use futures_util::StreamExt;
use massa_proto::massa::api::v1 as grpc;
use std::pin::Pin;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::error;

/// Type declaration for NewOperations
pub(crate) type NewOperationsStreamType = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc::NewOperationsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Creates a new stream of new produced and received operations
pub(crate) async fn new_operations(
    grpc: &MassaGrpc,
    request: Request<Streaming<grpc::NewOperationsRequest>>,
) -> Result<NewOperationsStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new operations channel
    let mut subscriber = grpc.pool_channels.operation_sender.subscribe();

    tokio::spawn(async move {
        if let Some(Ok(request)) = in_stream.next().await {
            let mut request_id = request.id;
            let mut filter = request.query.and_then(|q| q.filter);

            // Spawn a new task for sending new operations
            loop {
                select! {
                    // Receive a new operation from the subscriber
                     event = subscriber.recv() => {
                        match event {
                            Ok(operation) => {
                                // Check if the operation should be sent
                                if !should_send(&filter, operation.clone().content.op.into()) {
                                    continue;
                                }

                                // Convert the operation to a gRPC operation
                                let ret = grpc::SignedOperation {
                                    content: Some(operation.content.into()),
                                    signature: operation.signature.to_string(),
                                    content_creator_pub_key: operation.content_creator_pub_key.to_string(),
                                    content_creator_address: operation.content_creator_address.to_string(),
                                    id: operation.id.to_string()
                                };
                                // Send the new operation through the channel
                                if let Err(e) = tx.send(Ok(grpc::NewOperationsResponse {
                                    id: request_id.clone(),
                                    operation: Some(ret)
                                })).await {
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
                                        // Update current filter && request id
                                        filter = data.query
                                        .and_then(|q| q.filter);
                                        request_id = data.id;
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

fn should_send(filter_opt: &Option<grpc::NewOperationsFilter>, ope_type: grpc::OpType) -> bool {
    match filter_opt {
        Some(filter) => {
            let filtered_ope_ids = &filter.types;
            if filtered_ope_ids.is_empty() {
                true
            } else {
                let id: i32 = ope_type as i32;
                filtered_ope_ids.contains(&id)
            }
        }
        None => true, // if user has no filter = All operations type is send
    }
}
