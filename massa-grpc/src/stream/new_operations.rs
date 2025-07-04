// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaPublicGrpc;
use futures_util::StreamExt;
use massa_proto_rs::massa::api::v1::{self as grpc_api};
use std::{pin::Pin, time::Duration};
use tokio::{select, time};
use tonic::{Request, Streaming};
use tracing::error;

use super::trait_filters_impl::{FilterGrpc, FilterNewOperations};

/// Type declaration for NewOperations
pub type NewOperationsStreamType = Pin<
    Box<
        dyn futures_util::Stream<Item = Result<grpc_api::NewOperationsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Type declaration for NewOperations server
pub type NewOperationsServerStreamType = Pin<
    Box<
        dyn futures_util::Stream<
                Item = Result<grpc_api::NewOperationsServerResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;

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
            let mut filters =
                match FilterNewOperations::build_from_request(request.filters, &config) {
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
                                if let Some(data) = filters.filter_output(massa_operation, &config) {
                                         // Send the new operation through the channel
                                         if let Err(e) = tx.send(Ok(grpc_api::NewOperationsResponse {signed_operation: Some(data.into())})).await {
                                            error!("failed to send operation : {}", e);
                                            break;
                                        }
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
                                        filters = match FilterNewOperations::build_from_request(message.filters, &config) {
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

/// Creates a new stream of new produced and received operations
/// unidirectional streaming
pub(crate) async fn new_operations_server(
    grpc: &MassaPublicGrpc,
    request: Request<grpc_api::NewOperationsServerRequest>,
) -> Result<NewOperationsServerStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner request
    let request = request.into_inner();
    // Subscribe to the new operations channel
    let mut subscriber = grpc.pool_broadcasts.operation_sender.subscribe();
    // Clone grpc to be able to use it in the spawned task
    let config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        let filter = match FilterNewOperations::build_from_request(request.filters, &config) {
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

        // Create a timer that ticks every 10 seconds to check if the client is still connected
        let mut interval = time::interval(Duration::from_secs(
            config.unidirectional_stream_interval_check,
        ));

        // Continuously loop until the stream ends or an error occurs
        loop {
            select! {
                // Receive a new filled block from the subscriber
                event = subscriber.recv() => {
                    match event {
                        Ok(massa_operation) => {
                            // Check if the operation should be sent
                            if let Some(data) = filter.filter_output(massa_operation, &config) {
                                // Send the new operation through the channel
                                if let Err(e) = tx
                                    .send(Ok(grpc_api::NewOperationsServerResponse {
                                        signed_operation: Some(data.into()),
                                    }))
                                    .await
                                {
                                    error!("failed to send operation : {}", e);
                                    break;
                                }
                            }
                        }
                        Err(e) => error!("error on receive new operation: {}", e)
                    }
                },
                // Execute the code block whenever the timer ticks
                _ = interval.tick() => {
                    if tx.is_closed() {
                        // Client disconnected
                        break;
                    }
                }
            }
        }
    });

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as NewOperationsServerStreamType)
}
