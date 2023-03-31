// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaGrpc;
use futures_util::StreamExt;
use massa_models::block::{BlockDeserializer, BlockDeserializerArgs, SecureShareBlock};
use massa_models::error::ModelsError;
use massa_models::secure_share::SecureShareDeserializer;
use massa_proto::google::rpc::Status;
use massa_proto::massa::api::v1 as grpc;
use massa_serialization::{DeserializeError, Deserializer};
use std::io::ErrorKind;
use std::pin::Pin;
use tokio::sync::mpsc::Sender;
use tonic::codegen::futures_core;
use tonic::Request;
use tracing::log::{error, warn};

/// Type declaration for SendBlockStream
pub type SendBlocksStream = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc::SendBlocksStreamResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// This function takes a streaming request of block messages,
/// verifies, saves and propagates the block received in each message, and sends back a stream of
/// block id messages
pub(crate) async fn send_blocks(
    grpc: &MassaGrpc,
    request: Request<tonic::Streaming<grpc::SendBlocksStreamRequest>>,
) -> Result<SendBlocksStream, GrpcError> {
    let consensus_controller = grpc.consensus_controller.clone();
    let mut protocol_command_sender = grpc.protocol_command_sender.clone();
    let storage = grpc.storage.clone_without_refs();
    let config = grpc.grpc_config.clone();

    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(config.max_channel_size);
    // Extract the incoming stream of block messages
    let mut in_stream = request.into_inner();

    // Spawn a task that reads incoming messages and processes the block in each message
    tokio::spawn(async move {
        while let Some(result) = in_stream.next().await {
            match result {
                Ok(req_content) => {
                    let Some(proto_block) = req_content.block else {
                        report_error(
                            req_content.id.clone(),
                            tx.clone(),
                            tonic::Code::InvalidArgument,
                            "the request payload is empty".to_owned(),
                        ).await;
                        continue;
                    };

                    let args = BlockDeserializerArgs {
                        thread_count: config.thread_count,
                        max_operations_per_block: config.max_operations_per_block,
                        endorsement_count: config.endorsement_count,
                    };
                    // Deserialize and verify received block in the incoming message
                    match SecureShareDeserializer::new(BlockDeserializer::new(args))
                        .deserialize::<DeserializeError>(&proto_block.serialized_data)
                    {
                        Ok(tuple) => {
                            let (rest, res_block): (&[u8], SecureShareBlock) = tuple;
                            if rest.is_empty() {
                                if let Err(e) = res_block
                                    .verify_signature()
                                    .and_then(|_| res_block.content.header.verify_signature())
                                    .map(|_| {
                                        res_block
                                            .content
                                            .header
                                            .content
                                            .endorsements
                                            .iter()
                                            .map(|endorsement| endorsement.verify_signature())
                                            .collect::<Vec<Result<(), ModelsError>>>()
                                    })
                                {
                                    report_error(
                                        req_content.id.clone(),
                                        tx.clone(),
                                        tonic::Code::InvalidArgument,
                                        format!("wrong signature: {}", e),
                                    )
                                    .await;
                                    continue;
                                }

                                let block_id = res_block.id;
                                let slot = res_block.content.header.content.slot;
                                let mut block_storage = storage.clone_without_refs();

                                // Add the received block to the graph
                                block_storage.store_block(res_block.clone());
                                consensus_controller.register_block(
                                    block_id,
                                    slot,
                                    block_storage.clone(),
                                    false,
                                );

                                // Propagate the block(header) to the network
                                if let Err(e) = protocol_command_sender
                                    .integrated_block(block_id, block_storage)
                                {
                                    // If propagation failed, send an error message back to the client
                                    report_error(
                                        req_content.id.clone(),
                                        tx.clone(),
                                        tonic::Code::Internal,
                                        format!("failed to propagate block: {}", e),
                                    )
                                    .await;
                                    continue;
                                };

                                // Build the response message
                                let result = grpc::BlockResult {
                                    block_id: res_block.id.to_string(),
                                };
                                // Send the response message back to the client
                                if let Err(e) = tx
                                    .send(Ok(grpc::SendBlocksStreamResponse {
                                        id: req_content.id.clone(),

                                        result: Some(
                                            grpc::send_blocks_stream_response::Result::Ok(result),
                                        ),
                                    }))
                                    .await
                                {
                                    error!("failed to send back block response: {}", e);
                                };
                            } else {
                                report_error(
                                    req_content.id.clone(),
                                    tx.clone(),
                                    tonic::Code::InvalidArgument,
                                    "there is data left after operation deserialization".to_owned(),
                                )
                                .await;
                            }
                            continue;
                        }
                        // If the verification failed, send an error message back to the client
                        Err(e) => {
                            report_error(
                                req_content.id.clone(),
                                tx.clone(),
                                tonic::Code::InvalidArgument,
                                format!("failed to deserialize block: {}", e),
                            )
                            .await;
                            continue;
                        }
                    };
                }
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
                        error!("failed to send back send_blocks error response: {}", e);
                        break;
                    }
                }
            }
        }
    });

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as SendBlocksStream)
}

/// This function reports an error to the sender by sending a gRPC response message to the client
async fn report_error(
    id: String,
    sender: Sender<Result<grpc::SendBlocksStreamResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) {
    error!("{}", error);
    // Attempt to send the error response message to the sender
    if let Err(e) = sender
        .send(Ok(grpc::SendBlocksStreamResponse {
            id,
            result: Some(grpc::send_blocks_stream_response::Result::Error(Status {
                code: code.into(),
                message: error,
                details: Vec::new(),
            })),
        }))
        .await
    {
        // If sending the message fails, log the error message
        error!("failed to send back send_blocks error response: {}", e);
    }
}
