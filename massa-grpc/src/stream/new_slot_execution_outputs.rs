// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaPublicGrpc;
use futures_util::StreamExt;
use massa_proto_rs::massa::api::v1::{self as grpc_api};
use std::io::ErrorKind;
use std::pin::Pin;
use std::time::Duration;
use tokio::{select, time};
use tonic::{Request, Streaming};
use tracing::{error, warn};

use super::trait_filters_impl::{FilterGrpc, FilterNewSlotExec};

/// Type declaration for NewSlotExecutionOutputs
pub type NewSlotExecutionOutputsStreamType = Pin<
    Box<
        dyn futures_util::Stream<
                Item = Result<grpc_api::NewSlotExecutionOutputsResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;

/// Type declaration for NewSlotExecutionOutputsServer
pub type NewSlotExecutionOutputsServerStreamType = Pin<
    Box<
        dyn futures_util::Stream<
                Item = Result<grpc_api::NewSlotExecutionOutputsServerResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;

/// Creates a new stream of new produced and received slot execution outputs
pub(crate) async fn new_slot_execution_outputs(
    grpc: &MassaPublicGrpc,
    request: Request<Streaming<grpc_api::NewSlotExecutionOutputsRequest>>,
) -> Result<NewSlotExecutionOutputsStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new slot execution events channel
    let mut subscriber = grpc
        .execution_channels
        .slot_execution_output_sender
        .subscribe();
    let grpc_config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        if let Some(Ok(request)) = in_stream.next().await {
            let mut filters: FilterNewSlotExec = match FilterNewSlotExec::build_from_request(
                request.clone().filters,
                &grpc_config,
            ) {
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
                    // Receive a new slot execution output from the subscriber
                    event = subscriber.recv() => {
                        match event {
                            Ok(massa_slot_execution_output) => {
                                if let Some(data) = filters.filter_output(massa_slot_execution_output, &grpc_config) {
                                    if let Err(e) = tx.send(Ok(grpc_api::NewSlotExecutionOutputsResponse {
                                        output: Some(data.into()),
                                    })).await {
                                        error!("failed to send new slot execution output : {}", e);
                                        break;
                                    }
                                }
                            },

                            Err(e) => error!("error on receive new slot execution output : {}", e)
                        }
                    },
                    // Receive a new message from the in_stream
                    res = in_stream.next() => {
                        match res {
                            Some(res) => {
                                match res {
                                    Ok(message) => {
                                        // Update current filter
                                        filters = match FilterNewSlotExec::build_from_request(message.clone().filters, &grpc_config) {
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
                                    // Handle any errors that may occur during receiving the data
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
                                            error!("failed to send back new_slot_execution_outputs error response: {}", e);
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

    // Return the new stream of slot execution output
    Ok(Box::pin(out_stream) as NewSlotExecutionOutputsStreamType)
}

pub(crate) async fn new_slot_execution_outputs_server(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::NewSlotExecutionOutputsServerRequest>,
) -> Result<NewSlotExecutionOutputsServerStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Subscribe to the new slot execution events channel
    let mut subscriber = grpc
        .execution_channels
        .slot_execution_output_sender
        .subscribe();
    let grpc = grpc.clone();
    let inner_req = request.into_inner();
    tokio::spawn(async move {
        let filters: FilterNewSlotExec = match FilterNewSlotExec::build_from_request(
            inner_req.clone().filters,
            &grpc.grpc_config,
        ) {
            Ok(filter) => filter,
            Err(err) => {
                error!("failed to get filter: {}", err);
                // Send the error response back to the client
                if let Err(e) = tx.send(Err(err.into())).await {
                    error!("failed to send back error response: {}", e);
                }
                return;
            }
        };

        // Create a timer that ticks every 10 seconds to check if the client is still connected
        let mut interval = time::interval(Duration::from_secs(
            grpc.grpc_config.unidirectional_stream_interval_check,
        ));

        // Continuously loop until the stream ends or an error occurs
        loop {
            select! {
                // Receive a new filled block from the subscriber
                event = subscriber.recv() => {
                    match event {
                        Ok(massa_slot_execution_output) => {
                            // Check if the slot execution output should be sent
                            if let Some(slot_execution_output) =
                                filters.filter_output(massa_slot_execution_output, &grpc.grpc_config)
                            {
                                // Send the new slot execution output through the channel
                                if let Err(e) = tx
                                    .send(Ok(grpc_api::NewSlotExecutionOutputsServerResponse {
                                        output: Some(slot_execution_output.into()),
                                    }))
                                    .await
                                {
                                    error!("failed to send new slot execution output : {}", e);
                                    break;
                                }
                            }
                        }
                        Err(e) => error!("error on receive new slot execution output : {}", e),
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
    // Create a new stream from the received channel
    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    // Return the new stream of slot execution output
    Ok(Box::pin(out_stream) as NewSlotExecutionOutputsServerStreamType)
}
