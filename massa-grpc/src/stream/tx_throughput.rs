// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::{error::GrpcError, server::MassaPublicGrpc};
use futures_util::StreamExt;
use massa_proto_rs::massa::api::v1 as grpc_api;
use std::pin::Pin;
use std::time::Duration;
use tokio::{select, time};
use tracing::error;

/// default throughput interval in seconds
///
/// set 'high' value to avoid spamming the client with updates who doesn't need
///
/// end user can override this value by sending a request with a custom interval
const DEFAULT_THROUGHPUT_INTERVAL: u64 = 10;

/// Type declaration for TransactionsThroughput
pub type TransactionsThroughputStreamType = Pin<
    Box<
        dyn futures_util::Stream<
                Item = Result<grpc_api::TransactionsThroughputResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;

/// Type declaration for TransactionsThroughput
pub type TransactionsThroughputServerStreamType = Pin<
    Box<
        dyn futures_util::Stream<
                Item = Result<grpc_api::TransactionsThroughputServerResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;

/// The function returns a stream of transaction throughput statistics
pub(crate) async fn transactions_throughput(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<tonic::Streaming<grpc_api::TransactionsThroughputRequest>>,
) -> Result<TransactionsThroughputStreamType, GrpcError> {
    let execution_controller = grpc.execution_controller.clone();

    // Create a channel for sending responses to the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Extract the incoming stream of operations messages
    let mut in_stream = request.into_inner();

    // Spawn a new Tokio task to handle the stream processing
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(DEFAULT_THROUGHPUT_INTERVAL));

        // Continuously loop until the stream ends or an error occurs
        loop {
            select! {
                // Receive a new message from the in_stream
                res = in_stream.next() => {
                    match res {
                        Some(Ok(req)) => {
                            // Update the interval timer based on the request (or use the default)
                            let new_timer = req.interval.unwrap_or(DEFAULT_THROUGHPUT_INTERVAL);
                            interval = time::interval(Duration::from_secs(new_timer));
                            interval.reset();
                        },
                        _ => {
                            // Client disconnected
                            break;
                        }
                    }
                },
                // Execute the code block whenever the timer ticks
                _ = interval.tick() => {
                    let stats = execution_controller.get_stats();
                    // Calculate the throughput over the time window
                    let nb_sec_range = stats
                        .time_window_end
                        .saturating_sub(stats.time_window_start)
                        .to_duration()
                        .as_secs();
                    let throughput = stats
                        .final_executed_operations_count
                        .checked_div(nb_sec_range as usize)
                        .unwrap_or_default() as u32;
                    // Send the throughput response back to the client
                    if let Err(e) = tx
                        .send(Ok(grpc_api::TransactionsThroughputResponse {
                            throughput,
                        }))
                        .await
                    {
                        // Log an error if sending the response fails
                        error!("failed to send back transactions_throughput response: {}", e);
                        break;
                    }
                }
            }
        }
    });

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as TransactionsThroughputStreamType)
}

/// The function returns a stream unidirectional of transaction throughput statistics
pub(crate) async fn transactions_throughput_server(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::TransactionsThroughputServerRequest>,
) -> Result<TransactionsThroughputServerStreamType, GrpcError> {
    let execution_controller = grpc.execution_controller.clone();

    // Create a channel for sending responses to the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Extract the incoming stream of operations messages
    let request = request.into_inner();

    // Spawn a new Tokio task to handle the stream processing
    tokio::spawn(async move {
        let mut interval = if let Some(interval_user) = request.interval {
            time::interval(Duration::from_secs(interval_user))
        } else {
            time::interval(Duration::from_secs(DEFAULT_THROUGHPUT_INTERVAL))
        };

        // Continuously loop until the stream ends or an error occurs
        loop {
            // Execute the code block whenever the timer ticks
            interval.tick().await;

            let stats = execution_controller.get_stats();
            // Calculate the throughput over the time window
            let nb_sec_range = stats
                .time_window_end
                .saturating_sub(stats.time_window_start)
                .to_duration()
                .as_secs();
            let throughput = stats
                .final_executed_operations_count
                .checked_div(nb_sec_range as usize)
                .unwrap_or_default() as u32;
            // Send the throughput response back to the client
            if let Err(e) = tx
                .send(Ok(grpc_api::TransactionsThroughputServerResponse {
                    throughput,
                }))
                .await
            {
                // Log an error if sending the response fails
                error!(
                    "failed to send back transactions_throughput response: {}",
                    e
                );
                break;
            }
        }
    });

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as TransactionsThroughputServerStreamType)
}
