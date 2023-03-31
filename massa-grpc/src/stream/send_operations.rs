// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaGrpc;
use futures_util::StreamExt;
use massa_models::operation::{OperationDeserializer, SecureShareOperation};
use massa_models::secure_share::SecureShareDeserializer;
use massa_proto::massa::api::v1 as grpc;
use massa_serialization::{DeserializeError, Deserializer};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::pin::Pin;
use tonic::codegen::futures_core;
use tracing::log::{error, warn};

/// Type declaration for SendOperationsStream
pub type SendOperationsStream = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc::SendOperationsStreamResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// This function takes a streaming request of operations messages,
/// verifies, saves and propagates the operations received in each message, and sends back a stream of
/// operations ids messages
pub(crate) async fn send_operations(
    grpc: &MassaGrpc,
    request: tonic::Request<tonic::Streaming<grpc::SendOperationsStreamRequest>>,
) -> Result<SendOperationsStream, GrpcError> {
    let mut pool_command_sender = grpc.pool_command_sender.clone();
    let mut protocol_command_sender = grpc.protocol_command_sender.clone();
    let config = grpc.grpc_config.clone();
    let storage = grpc.storage.clone_without_refs();

    // Create a channel for sending responses to the client
    let (tx, rx) = tokio::sync::mpsc::channel(config.max_channel_size);
    // Extract the incoming stream of operations messages
    let mut in_stream = request.into_inner();

    // Spawn a task that reads incoming messages and processes the operations in each message
    tokio::spawn(async move {
        while let Some(result) = in_stream.next().await {
            match result {
                Ok(req_content) => {
                    // If the incoming message has no operations, send an error message back to the client
                    if req_content.operations.is_empty() {
                        report_error(
                            req_content.id.clone(),
                            tx.clone(),
                            tonic::Code::InvalidArgument,
                            "the request payload is empty".to_owned(),
                        )
                        .await;
                    } else {
                        // If there are too many operations in the incoming message, send an error message back to the client
                        if req_content.operations.len() as u32 > config.max_operations_per_message {
                            report_error(
                                req_content.id.clone(),
                                tx.clone(),
                                tonic::Code::InvalidArgument,
                                "too many operations per message".to_owned(),
                            )
                            .await;
                        } else {
                            // Deserialize and verify each operation in the incoming message
                            let operation_deserializer =
                                SecureShareDeserializer::new(OperationDeserializer::new(
                                    config.max_datastore_value_length,
                                    config.max_function_name_length,
                                    config.max_parameter_size,
                                    config.max_op_datastore_entry_count,
                                    config.max_op_datastore_key_length,
                                    config.max_op_datastore_value_length,
                                ));
                            let verified_ops_res: Result<HashMap<String, SecureShareOperation>, GrpcError> = req_content.operations
                                .into_iter()
                                .map(|proto_operation| {
                                    // Concatenate signature, public key, and data into a single byte vector
                                    let mut op_serialized = Vec::new();
                                    op_serialized.extend(proto_operation.signature.as_bytes());
                                    op_serialized.extend(proto_operation.content_creator_pub_key.as_bytes());
                                    op_serialized.extend(proto_operation.serialized_data);

                                    // Deserialize the operation and verify its signature
                                    let verified_op_res = match operation_deserializer.deserialize::<DeserializeError>(&op_serialized) {
                                        Ok(tuple) => {
                                            let (rest, res_operation): (&[u8], SecureShareOperation) = tuple;
                                            if rest.is_empty() {
                                                res_operation.verify_signature()
                                                    .map(|_| (res_operation.id.to_string(), res_operation))
                                                    .map_err(|e| e.into())
                                            } else {
                                                Err(GrpcError::InternalServerError(
                                                    "there is data left after operation deserialization".to_owned()
                                                ))
                                            }
                                        }
                                        Err(e) => {
                                            Err(GrpcError::InternalServerError(format!("failed to deserialize operation: {}", e)))
                                        }
                                    };
                                    verified_op_res
                                })
                                .collect();

                            match verified_ops_res {
                                // If all operations in the incoming message are valid, store and propagate them
                                Ok(verified_ops) => {
                                    let mut operation_storage = storage.clone_without_refs();
                                    operation_storage
                                        .store_operations(verified_ops.values().cloned().collect());
                                    // Add the received operations to the operations pool
                                    pool_command_sender.add_operations(operation_storage.clone());

                                    // Propagate the operations to the network
                                    if let Err(e) = protocol_command_sender
                                        .propagate_operations(operation_storage)
                                    {
                                        // If propagation failed, send an error message back to the client
                                        let error =
                                            format!("failed to propagate operations: {}", e);
                                        report_error(
                                            req_content.id.clone(),
                                            tx.clone(),
                                            tonic::Code::Internal,
                                            error.to_owned(),
                                        )
                                        .await;
                                    };

                                    // Build the response message
                                    let result = grpc::OperationResult {
                                        operations_ids: verified_ops.keys().cloned().collect(),
                                    };
                                    // Send the response message back to the client
                                    if let Err(e) = tx
                                        .send(Ok(grpc::SendOperationsStreamResponse {
                                            id: req_content.id.clone(),
                                            message: Some(
                                                grpc::send_operations_stream_response::Message::Result(
                                                    result,
                                                ),
                                            ),
                                        }))
                                        .await
                                    {
                                        error!("failed to send back operations response: {}", e);
                                    };
                                }
                                // If the verification failed, send an error message back to the client
                                Err(e) => {
                                    let error = format!("invalid operation(s): {}", e);
                                    report_error(
                                        req_content.id.clone(),
                                        tx.clone(),
                                        tonic::Code::InvalidArgument,
                                        error.to_owned(),
                                    )
                                    .await;
                                }
                            }
                        }
                    }
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
                        error!("failed to send back send_operations error response: {}", e);
                        break;
                    }
                }
            }
        }
    });

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as SendOperationsStream)
}

/// This function reports an error to the sender by sending a gRPC response message to the client
async fn report_error(
    id: String,
    sender: tokio::sync::mpsc::Sender<Result<grpc::SendOperationsStreamResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) {
    error!("{}", error);
    // Attempt to send the error response message to the sender
    if let Err(e) = sender
        .send(Ok(grpc::SendOperationsStreamResponse {
            id,
            message: Some(grpc::send_operations_stream_response::Message::Error(
                massa_proto::google::rpc::Status {
                    code: code.into(),
                    message: error,
                    details: Vec::new(),
                },
            )),
        }))
        .await
    {
        // If sending the message fails, log the error message
        error!("failed to send back send_operations error response: {}", e);
    }
}
