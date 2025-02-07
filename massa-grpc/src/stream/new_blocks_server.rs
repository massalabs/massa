use tonic::Request;

use super::{
    new_blocks::NewBlocksStreamType,
    tools::{get_filter_new_blocks, should_send_new_blocks},
};
use crate::{error::GrpcError, server::MassaPublicGrpc};
use massa_proto_rs::massa::api::v1 as grpc_api;
use tracing::log::error;

/// Creates a new stream of new produced and received blocks
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
        let filters = match get_filter_new_blocks(request, &grpc_config) {
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
                    if !should_send_new_blocks(&massa_block, &filters, &grpc_config) {
                        continue;
                    }
                    // Send the new block through the channel
                    if let Err(e) = tx
                        .send(Ok(grpc_api::NewBlocksResponse {
                            signed_block: Some(massa_block.into()),
                        }))
                        .await
                    {
                        error!("failed to send new block : {}", e);
                        break;
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
