use crate::error::{match_for_io_error, GrpcError};
use crate::service::MassaGrpcService;
use futures_util::StreamExt;
use massa_models::block::FilledBlock;
use massa_proto::massa::api::v1::{NewFilledBlocksStreamRequest, NewFilledBlocksStreamResponse};
use std::io::ErrorKind;
use std::pin::Pin;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::{error, warn};

/// Type declaration for NewFilledBlocksStream
pub type NewFilledBlocksStream = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<NewFilledBlocksStreamResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Creates a new stream of new produced and received filled blocks
pub(crate) async fn new_filled_blocks(
    grpc: &MassaGrpcService,
    request: Request<Streaming<NewFilledBlocksStreamRequest>>,
) -> Result<NewFilledBlocksStream, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new filled blocks channel
    let mut subscriber = grpc.consensus_channels.filled_block_sender.subscribe();

    // Spawn a new task for sending new blocks
    tokio::spawn(async move {
        // Initialize the request_id string
        let mut request_id = String::new();
        loop {
            select! {
                // Receive a new filled block from the subscriber
                 event = subscriber.recv() => {
                    match event {
                        Ok(share_block) => {
                            let massa_block = share_block as FilledBlock;
                            // Send the new filled block through the channel
                            if let Err(e) = tx.send(Ok(NewFilledBlocksStreamResponse {
                                    id: request_id.clone(),
                                    filled_block: Some(massa_block.into())
                            })).await {
                                error!("failed to send new block : {}", e);
                                break;
                            }
                        },
                        Err(e) => {error!("error on receive new block : {}", e)}
                    }
                },
            // Receive a new request from the in_stream
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
                                    error!("failed to send back new_filled_blocks error response: {}", e);
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

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as NewFilledBlocksStream)
}
