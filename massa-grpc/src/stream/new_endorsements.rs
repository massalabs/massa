// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaGrpc;
use futures_util::StreamExt;
use massa_proto_rs::massa::api::v1 as grpc_api;
use std::io::ErrorKind;
use std::pin::Pin;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::{error, warn};

/// Type declaration for NewEndorsements
pub type NewEndorsementsStreamType = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc_api::NewEndorsementsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Creates a new stream of new produced and received endorsements
pub(crate) async fn new_endorsements(
    grpc: &MassaGrpc,
    request: Request<Streaming<grpc_api::NewEndorsementsRequest>>,
) -> Result<NewEndorsementsStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new endorsements channel
    let mut subscriber = grpc.pool_channels.endorsement_sender.subscribe();

    tokio::spawn(async move {
        loop {
            select! {
                // Receive a new endorsement from the subscriber
                event = subscriber.recv() => {
                    match event {
                        Ok(massa_endorsement) => {
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
                            if let Err(err) = res {
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
                                        error!("failed to send back new_endorsements error response: {}", e);
                                        break;
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
    });

    // Create a new stream from the received channel
    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    // Return the new stream of endorsements
    Ok(Box::pin(out_stream) as NewEndorsementsStreamType)
}
