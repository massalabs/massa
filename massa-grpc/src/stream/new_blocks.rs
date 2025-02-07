// Copyright (c) 2025 MASSA LABS <info@massa.net>

use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaPublicGrpc;
use futures_util::StreamExt;
use massa_proto_rs::massa::api::v1::{self as grpc_api};
use std::io::ErrorKind;
use std::pin::Pin;
use tokio::select;
use tonic::{Request, Streaming};
use tracing::{error, warn};

use super::trait_filters_impl::{FilterGrpc, FilterNewBlocks};

/// Type declaration for NewBlocks
pub type NewBlocksStreamType = Pin<
    Box<
        dyn futures_util::Stream<Item = Result<grpc_api::NewBlocksResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Creates a new stream of new produced and received blocks
pub(crate) async fn new_blocks(
    grpc: &MassaPublicGrpc,
    request: Request<Streaming<grpc_api::NewBlocksRequest>>,
) -> Result<NewBlocksStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new blocks channel
    let mut subscriber = grpc.consensus_broadcasts.block_sender.subscribe();
    // Clone grpc to be able to use it in the spawned task
    let grpc_config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        if let Some(Ok(request)) = in_stream.next().await {
            let mut filters = match FilterNewBlocks::build_from_request(request, &grpc_config) {
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
                    // Receive a new block from the subscriber
                    event = subscriber.recv() => {
                        match event {
                            Ok(massa_block) => {
                                // Check if the block should be sent
                                if let Some(data) = filters.filter_output(massa_block, &grpc_config) {
                                    // Send the new block through the channel
                                    if let Err(e) = tx.send(Ok(grpc_api::NewBlocksResponse {
                                        signed_block: Some(data.into())
                                    })).await {
                                        error!("failed to send new block : {}", e);
                                        break;
                                    }
                                }

                            },
                            Err(e) => error!("error on receive new block : {}", e)
                        }
                    },
                    res = in_stream.next() => {
                        match res {
                            Some(res) => {
                                match res {
                                    Ok(message) => {
                                        // Update current filter
                                        filters = match FilterNewBlocks::build_from_request(message, &grpc_config) {
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
                                            error!("failed to send back NewBlocks error response: {}", e);
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

    // Return the new stream of blocks
    Ok(Box::pin(out_stream) as NewBlocksStreamType)
}

/// Creates a new stream of new produced and received blocks
/// uni-directional streaming
pub(crate) async fn new_blocks_server(
    grpc: &MassaPublicGrpc,
    request: Request<grpc_api::NewBlocksRequest>,
) -> Result<NewBlocksStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let request = request.into_inner();
    // Subscribe to the new blocks channel
    let mut subscriber = grpc.consensus_broadcasts.block_sender.subscribe();
    // Clone grpc to be able to use it in the spawned task
    let grpc_config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        // let filters = match get_filter_new_blocks(request, &grpc_config) {
        let filter = match FilterNewBlocks::build_from_request(request, &grpc_config) {
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
            // Receive a new block from the subscriber
            match subscriber.recv().await {
                Ok(massa_block) => {
                    // Check if the block should be sent
                    if let Some(data) = filter.filter_output(massa_block, &grpc_config) {
                        // Send the new block through the channel
                        if let Err(e) = tx
                            .send(Ok(grpc_api::NewBlocksResponse {
                                signed_block: Some(data.into()),
                            }))
                            .await
                        {
                            error!("failed to send new block : {}", e);
                            break;
                        }
                    }
                }
                Err(e) => error!("error on receive new block : {}", e),
            }
        }
    });

    // Create a new stream from the received channel
    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    // Return the new stream of blocks
    Ok(Box::pin(out_stream) as NewBlocksStreamType)
}
