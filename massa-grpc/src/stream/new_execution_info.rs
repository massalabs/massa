use massa_proto_rs::massa::api::v1::{self as grpc_api};
#[cfg(feature = "execution-info")]
use tonic::Request;

use std::pin::Pin;

#[cfg(feature = "execution-info")]
use crate::{error::GrpcError, server::MassaPublicGrpc};

#[cfg(feature = "execution-info")]
use super::trait_filters_impl::{FilterGrpc, NewExecutionInfoFilter};

/// Type declaration for New execution Info server
pub type NewTransferInfoServerStreamType = Pin<
    Box<
        dyn futures_util::Stream<
                Item = Result<grpc_api::NewTransfersInfoServerResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;

#[cfg(feature = "execution-info")]
pub(crate) async fn new_transfer_info_server(
    grpc: &MassaPublicGrpc,
    request: Request<grpc_api::NewTransfersInfoServerRequest>,
) -> Result<NewTransferInfoServerStreamType, GrpcError> {
    use std::time::Duration;
    use tokio::{select, time};
    use tracing::error;

    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner request
    let request = request.into_inner();
    // Subscribe to the new operations channel
    let mut subscriber = grpc
        .execution_channels
        .slot_execution_info_sender
        .subscribe();
    // Clone grpc to be able to use it in the spawned task
    let config = grpc.grpc_config.clone();

    tokio::spawn(async move {
        let filter = match NewExecutionInfoFilter::build_from_request(request.address, &config) {
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
        // otherwise the server has no way to check if client has disconnected (and can help to save some resources)
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
                                    .send(Ok(grpc_api::NewTransfersInfoServerResponse::from(data)))
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
    Ok(Box::pin(out_stream) as NewTransferInfoServerStreamType)
}
