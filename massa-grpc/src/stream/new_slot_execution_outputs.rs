// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaGrpc;
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
    grpc: &MassaGrpc,
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
            let mut request_id = request.id;
            let mut filter = request.query.and_then(|q| q.filter);
            loop {
                select! {
                    // Receive a new slot execution output from the subscriber
                    event = subscriber.recv() => {
                        match event {
                            Ok(massa_slot_execution_output) => {
                                // Check if the slot execution output should be sent
                                if !should_send(&filter, &massa_slot_execution_output) {
                                  continue;
                                }
                                // Send the new slot execution output through the channel
                                if let Err(e) = tx.send(Ok(grpc_api::NewSlotExecutionOutputsResponse {
                                        id: request_id.clone(),
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
                                        filter = data.query
                                            .and_then(|q| q.filter);
                                        request_id = data.id
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
    filter_opt: &Option<grpc_api::NewSlotExecutionOutputsFilter>,
    exec_out_status: &SlotExecutionOutput,
) -> bool {
    match filter_opt {
        Some(filter) => match exec_out_status {
            SlotExecutionOutput::ExecutedSlot(_) => {
                let id = grpc_model::ExecutionOutputStatus::Candidate as i32;
                filter.status.contains(&id)
            }
            SlotExecutionOutput::FinalizedSlot(_) => {
                let id = grpc_model::ExecutionOutputStatus::Final as i32;
                filter.status.contains(&id)
            }
        },
        None => true, // if user has no filter = All execution outputs status are sent
    }
}
