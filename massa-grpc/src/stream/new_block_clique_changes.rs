// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaGrpc;
use futures_util::StreamExt;
use massa_proto::massa::api::v1 as grpc;
use std::io::ErrorKind;
use std::pin::Pin;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::{error, warn};

/// Type declaration for NewBlockCliqueChanges
pub type NewBlockCliqueChangesStreamType = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc::NewBlockCliqueChangesResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Creates a new stream of new produced and received blockclique changes
pub(crate) async fn new_block_clique_changes(
    grpc: &MassaGrpc,
    request: Request<Streaming<grpc::NewBlockCliqueChangesRequest>>,
) -> Result<NewBlockCliqueChangesStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new blockclique channel
    let mut subscriber = grpc.consensus_channels.block_clique_sender.subscribe();

    tokio::spawn(async move {
        // Initialize the request_id string
        let mut request_id = String::new();
        loop {
            select! {
                // Receive a new blockclique changes from the subscriber
                event = subscriber.recv() => {
                    match event {
                        Ok(massa_blockclique) => {
                            // Send the new blockclique changes through the channel
                            if let Err(e) = tx.send(Ok(grpc::NewBlockCliqueChangesResponse {
                                    id: request_id.clone(),
                                    state: Some(grpc::BlockCliqueChange {
                                        changes: massa_blockclique.into_iter().map(|(k,v)| grpc::BlockCliqueStateEntry {
                                           block_id: v.to_string(),
                                           slot: Some(k.into()),
                                           }
                                        )
                                            .collect(),
                                    }),
                            })).await {
                                error!("failed to send new blockclique changes: {}", e);
                                break;
                            }
                        },
                        Err(e) => {error!("error on receive new blockclique changes: {}", e)}
                    }
                },
                // Receive a new message from the in_stream
                res = in_stream.next() => {
                    match res {
                        Some(res) => {
                            match res {
                                // Get the request_id from the received data
                                Ok(data) => {
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
                                        error!("failed to send back new_block_clique_changes error response: {}", e);
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
    });

    // Create a new stream from the received channel.
    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    // Return the new stream of blockclique.
    Ok(Box::pin(out_stream) as NewBlockCliqueChangesStreamType)
}
