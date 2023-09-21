use crate::{error::GrpcError, server::MassaPublicGrpc};
use massa_proto_rs::massa::api::v1 as grpc_api;
use tracing::log::error;

use super::new_slot_execution_outputs::NewSlotExecutionOutputsStreamType;

pub(crate) async fn new_slot_execution_outputs_server(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<grpc_api::NewSlotExecutionOutputsRequest>,
) -> Result<NewSlotExecutionOutputsStreamType, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);

    // Subscribe to the new slot execution events channel
    let mut subscriber = grpc
        .execution_channels
        .slot_execution_output_sender
        .subscribe();

    let grpc = grpc.clone();

    let inner_req = request.into_inner();

    tokio::spawn(async move {
        let filters: super::new_slot_execution_outputs::Filter =
            match super::new_slot_execution_outputs::get_filter(
                inner_req.clone(),
                &grpc.grpc_config,
            ) {
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
            // Receive a new slot execution output from the subscriber
            match subscriber.recv().await {
                Ok(massa_slot_execution_output) => {
                    // todo
                    let slot_execution_output = super::new_slot_execution_outputs::filter_map(
                        massa_slot_execution_output,
                        &filters,
                        &grpc.grpc_config,
                    );
                    // Check if the slot execution output should be sent
                    if let Some(slot_execution_output) = slot_execution_output {
                        // Send the new slot execution output through the channel
                        if let Err(e) = tx
                            .send(Ok(grpc_api::NewSlotExecutionOutputsResponse {
                                output: Some(slot_execution_output.into()),
                            }))
                            .await
                        {
                            error!("failed to send new slot execution output : {}", e);
                            break;
                        }
                    }
                }

                Err(e) => error!("error on receive new slot execution output : {}", e),
            }
        }
    });

    // Create a new stream from the received channel
    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    // Return the new stream of slot execution output
    Ok(Box::pin(out_stream) as NewSlotExecutionOutputsStreamType)
}
