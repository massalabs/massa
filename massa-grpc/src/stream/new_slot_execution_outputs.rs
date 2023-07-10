// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaPublicGrpc;
use futures_util::StreamExt;
use massa_execution_exports::SlotExecutionOutput;
use massa_proto_rs::massa::api::v1 as grpc_api;
use massa_proto_rs::massa::model::v1 as grpc_model;
use std::io::ErrorKind;
use std::pin::Pin;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::{error, warn};

/// Type declaration for NewSlotExecutionOutputs
pub type NewSlotExecutionOutputsStreamType = Pin<
    Box<
        dyn futures_core::Stream<
                Item = Result<grpc_api::NewSlotExecutionOutputsResponse, tonic::Status>,
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

    tokio::spawn(async move {
        // Initialize the request_id string
        if let Some(Ok(request)) = in_stream.next().await {
            let mut filters: Vec<grpc_api::NewSlotExecutionOutputsFilter> = request.filters;

            loop {
                select! {
                    // Receive a new slot execution output from the subscriber
                    event = subscriber.recv() => {
                        match event {
                            Ok(massa_slot_execution_output) => {
                                // Check if the slot execution output should be sent
                                if !should_send(filters.clone(), &massa_slot_execution_output) {
                                  continue;
                                }
                                // Send the new slot execution output through the channel
                                if let Err(e) = tx.send(Ok(grpc_api::NewSlotExecutionOutputsResponse {
                                        output: Some(massa_slot_execution_output.into())
                                })).await {
                                    error!("failed to send new slot execution output : {}", e);
                                    break;
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
                                    // Get the request_id from the received data
                                    Ok(data) => {
                                        // Update current filter && request id
                                        filters = data.filters;
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

/// Return if the execution outputs should be send to client
fn should_send(
    filters: Vec<grpc_api::NewSlotExecutionOutputsFilter>,
    exec_out_status: &SlotExecutionOutput,
) -> bool {
    if filters.is_empty() {
        return true;
    }
    match exec_out_status {
        SlotExecutionOutput::ExecutedSlot(_) => {
            let id = grpc_model::ExecutionOutputStatus::Candidate as i32;
            filters.iter().any(|f| {
                matches!(f.filter.as_ref(), Some(grpc_api::new_slot_execution_outputs_filter::Filter::Status(status)) if *status == id)
            })
        }
        SlotExecutionOutput::FinalizedSlot(_) => {
            let id = grpc_model::ExecutionOutputStatus::Final as i32;
            filters.iter().any(|f| {
                matches!(f.filter.as_ref(), Some(grpc_api::new_slot_execution_outputs_filter::Filter::Status(status)) if *status == id)
            })
        }
    }
}
