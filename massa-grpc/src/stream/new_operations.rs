// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaGrpc;
use futures_util::StreamExt;
use massa_models::operation::OperationId;
use massa_proto_rs::massa::api::v1::{self as grpc_api, NewOperationsRequest};
use massa_proto_rs::massa::model::v1 as grpc_model;
use std::pin::Pin;
use std::str::FromStr;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::{error, warn};

/// Type declaration for NewOperations
pub type NewOperationsStreamType = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc_api::NewOperationsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

fn get_request_parameters(request: NewOperationsRequest) -> (Vec<OperationId>, Vec<i32>) {
    let mut operations_ids = Vec::new();
    let mut filter_ope_types = Vec::new();

    request.filters.into_iter().for_each(|query| {
        if let Some(filter) = query.filter {
            match filter {
                grpc_api::new_operations_filter::Filter::OperationIds(ids) => {
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

                    operations_ids.extend(ids.iter().map(|id| *id));
                }
                grpc_api::new_operations_filter::Filter::OperationTypes(ope_types) => {
                    filter_ope_types.extend_from_slice(&ope_types.op_types)
                }
            }
        }
    });
    (operations_ids, filter_ope_types)
}

/// Creates a new stream of new produced and received operations
pub(crate) async fn new_operations(
    grpc: &MassaGrpc,
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
            let mut request_id = request.id;
            let mut filter = request.query.and_then(|q| q.filter);

            // Spawn a new task for sending new operations
            loop {
                select! {
                    // Receive a new operation from the subscriber
                     event = subscriber.recv() => {
                        match event {
                            Ok(massa_operation) => {
                                // Check if the operation should be sent
                                if !should_send(&filter, massa_operation.clone().content.op.into()) {
                                    continue;
                                }

                                // Send the new operation through the channel
                                if let Err(e) = tx.send(Ok(grpc_api::NewOperationsResponse {
                                    id: request_id.clone(),
                                    operation: Some(massa_operation.into())
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

fn should_send(
    filter_opt: &Option<grpc_api::NewOperationsFilter>,
    ope_type: grpc_api::OpType,
) -> bool {
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
