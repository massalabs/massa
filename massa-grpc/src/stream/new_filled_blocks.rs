// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaPublicGrpc;
use crate::stream::trait_filters_impl::FilterGrpc;
use futures_util::StreamExt;
use massa_proto_rs::massa::api::v1 as grpc_api;
use std::io::ErrorKind;
use std::pin::Pin;
use std::time::Duration;
use tokio::{select, time};
use tonic::{Request, Streaming};
use tracing::{error, warn};

use super::trait_filters_impl::FilterNewFilledBlocks;

/// Type declaration for NewFilledBlocks
pub type NewFilledBlocksStreamType = Pin<
    Box<
        dyn futures_util::Stream<Item = Result<grpc_api::NewFilledBlocksResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Type declaration for NewFilledBlocksServerStream
pub type NewFilledBlocksServerStreamType = Pin<
    Box<
        dyn futures_util::Stream<
                Item = Result<grpc_api::NewFilledBlocksServerResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;

/// Creates a new stream of new produced and received filled blocks
pub(crate) async fn new_filled_blocks(
    grpc: &MassaPublicGrpc,
    request: Request<Streaming<grpc_api::NewFilledBlocksRequest>>,
) -> Result<NewFilledBlocksStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new filled blocks channel
    let mut subscriber = grpc.consensus_broadcasts.filled_block_sender.subscribe();
    // Clone grpc to be able to use it in the spawned task
    let grpc_config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        if let Some(Ok(request)) = in_stream.next().await {
            let mut filters =
                match FilterNewFilledBlocks::build_from_request(request.filters, &grpc_config) {
                    Ok(filter) => filter,
                    Err(err) => {
                        error!("failed to get filter: {}", err);
                        // Send the error response back to the client
                        if let Err(e) = tx.send(Err(err.into())).await {
                            error!("failed to send back NewFilledBlocks error response: {}", e);
                        }
                        return;
                    }
                };

            loop {
                select! {
                    // Receive a new filled block from the subscriber
                     event = subscriber.recv() => {
                        match event {
                            Ok(massa_filled_block) => {
                                // Check if the block should be sent
                                if let Some(data) = filters.filter_output(massa_filled_block, &grpc_config) {
                                    if let Err(e) = tx.send(Ok(grpc_api::NewFilledBlocksResponse {
                                        filled_block: Some(data.into())
                                    })).await {
                                        error!("failed to send new filled block : {}", e);
                                        break;
                                    }
                                }

                            },
                            Err(e) => error!("error on receive new filled block : {}", e)
                        }
                    },
                // Receive a new message from the in_stream
                res = in_stream.next() => {
                    match res {
                        Some(res) => {
                            match res {
                                Ok(message) => {
                                    // Update current filter
                                    filters = match FilterNewFilledBlocks::build_from_request(message.filters, &grpc_config) {
                                        Ok(filter) => filter,
                                        Err(err) => {
                                            error!("failed to get filter: {}", err);
                                            // Send the error response back to the client
                                            if let Err(e) = tx.send(Err(err.into())).await {
                                                error!("failed to send back NewFilledBlocks error response: {}", e);
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
                                        error!("failed to send back NewFilledBlocks error response: {}", e);
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

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as NewFilledBlocksStreamType)
}

/// Creates a new unidirectional stream of new produced and received filled blocks
pub(crate) async fn new_filled_blocks_server(
    grpc: &MassaPublicGrpc,
    request: Request<grpc_api::NewFilledBlocksServerRequest>,
) -> Result<NewFilledBlocksServerStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner the request
    let request = request.into_inner();
    // Subscribe to the new filled blocks channel
    let mut subscriber = grpc.consensus_broadcasts.filled_block_sender.subscribe();
    // Clone grpc to be able to use it in the spawned task
    let grpc_config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        let filter = match FilterNewFilledBlocks::build_from_request(request.filters, &grpc_config)
        {
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

        // Create a timer that ticks every 10 seconds to check if the client is still connected
        let mut interval = time::interval(Duration::from_secs(
            grpc_config.unidirectional_stream_interval_check,
        ));

        // Continuously loop until the stream ends or an error occurs
        loop {
            select! {
                // Receive a new filled block from the subscriber
                event = subscriber.recv() => {
                    match event {
                       Ok(massa_filled_block) => {
                           // Check if the block should be sent
                           if let Some(data) = filter.filter_output(massa_filled_block, &grpc_config) {
                               if let Err(e) = tx
                                   .send(Ok(grpc_api::NewFilledBlocksServerResponse {
                                       filled_block: Some(data.into()),
                                   }))
                                   .await
                               {
                                   error!("failed to send new filled block : {}", e);
                                   break;
                               }
                           }
                       }
                       Err(e) => error!("error on receive new filled block : {}", e),
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
    Ok(Box::pin(out_stream) as NewFilledBlocksServerStreamType)
}
