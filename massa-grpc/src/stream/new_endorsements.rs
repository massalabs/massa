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

use super::trait_filters_impl::{FilterGrpc, NewEndorsementsFilter};

/// Type declaration for NewEndorsements
pub type NewEndorsementsStreamType = Pin<
    Box<
        dyn futures_util::Stream<Item = Result<grpc_api::NewEndorsementsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Creates a new stream of new produced and received endorsements
pub(crate) async fn new_endorsements(
    grpc: &MassaPublicGrpc,
    request: Request<Streaming<grpc_api::NewEndorsementsRequest>>,
) -> Result<NewEndorsementsStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new endorsements channel
    let mut subscriber = grpc.pool_broadcasts.endorsement_sender.subscribe();
    // Clone grpc to be able to use it in the spawned task
    let grpc_config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        if let Some(Ok(request)) = in_stream.next().await {
            let mut filter = match NewEndorsementsFilter::build_from_request(request, &grpc_config)
            {
                Ok(filter) => filter,
                Err(err) => {
                    error!("failed to get filter: {}", err);
                    // Send the error response back to the client
                    if let Err(e) = tx.send(Err(err.into())).await {
                        error!("failed to send back NewEndorsements error response: {}", e);
                    }
                    return;
                }
            };

            loop {
                select! {
                    // Receive a new endorsement from the subscriber
                    event = subscriber.recv() => {
                        match event {
                            Ok(massa_endorsement) => {
                                // Check if the endorsement should be sent
                                if let Some(data) = filter.filter_output(massa_endorsement, &grpc_config) {
                                    // Send the new endorsement through the channel
                                    if let Err(e) = tx.send(Ok(grpc_api::NewEndorsementsResponse {
                                        signed_endorsement: Some(data.into())
                                    })).await {
                                        error!("failed to send new endorsement : {}", e);
                                        break;
                                    }
                                }
                            },
                            Err(e) => error!("error on receive new endorsement : {}", e)
                        }
                    },
                    // Receive a new message from the in_stream
                    res = in_stream.next() => {
                        match res {
                            Some(res) => {
                                match res {
                                    Ok(message) => {
                                        // Update current filter
                                        filter = match NewEndorsementsFilter::build_from_request(message, &grpc_config) {
                                            Ok(filter) => filter,
                                            Err(err) => {
                                                error!("failed to get filter: {}", err);
                                                // Send the error response back to the client
                                                if let Err(e) = tx.send(Err(err.into())).await {
                                                    error!("failed to send back NewEndorsements error response: {}", e);
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
                                            error!("failed to send back NewEndorsements error response: {}", e);
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

    // Return the new stream of endorsements
    Ok(Box::pin(out_stream) as NewEndorsementsStreamType)
}

/// Creates a new unidirectional stream of new produced and received endorsements
pub(crate) async fn new_endorsements_server(
    grpc: &MassaPublicGrpc,
    request: Request<grpc_api::NewEndorsementsRequest>,
) -> Result<NewEndorsementsStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner request
    let request = request.into_inner();
    // Subscribe to the new endorsements channel
    let mut subscriber = grpc.pool_broadcasts.endorsement_sender.subscribe();
    // Clone grpc to be able to use it in the spawned task
    let grpc_config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        let filter = match NewEndorsementsFilter::build_from_request(request, &grpc_config) {
            Ok(filter) => filter,
            Err(err) => {
                error!("failed to get filter: {}", err);
                // Send the error response back to the client
                if let Err(e) = tx.send(Err(err.into())).await {
                    error!("failed to send back new endorsement error response: {}", e);
                }
                return;
            }
        };

        // Create a timer that ticks every 10 seconds to check if the client is still connected
        let mut interval = time::interval(Duration::from_secs(
            grpc_config.unidirectional_stream_interval_check,
        ));

        loop {
            select! {
                // Receive a new endorsement from the subscriber
                event = subscriber.recv() => {
                    match event {
                        Ok(massa_endorsement) => {
                            // Check if the endorsement should be sent
                            if let Some(data) = filter.filter_output(massa_endorsement, &grpc_config) {
                                // Send the new endorsement through the channel
                                if let Err(e) = tx.send(Ok(grpc_api::NewEndorsementsResponse {
                                    signed_endorsement: Some(data.into())
                                })).await {
                                    error!("failed to send new endorsement : {}", e);
                                    break;
                                }
                            }
                        },
                        Err(e) => error!("error on receive new endorsement : {}", e)
                    }
                },
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

    // Return the new stream of endorsements
    Ok(Box::pin(out_stream) as NewEndorsementsStreamType)
}
