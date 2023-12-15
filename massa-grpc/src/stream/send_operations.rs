// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaPublicGrpc;
use futures_util::StreamExt;
use massa_models::operation::{OperationDeserializer, OperationType, SecureShareOperation};
use massa_models::secure_share::SecureShareDeserializer;
use massa_models::timeslots::get_latest_block_slot_at_timestamp;
use massa_proto_rs::massa::api::v1 as grpc_api;
use massa_proto_rs::massa::model::v1 as grpc_model;
use massa_serialization::{DeserializeError, Deserializer};
use massa_time::MassaTime;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::pin::Pin;
use tracing::{error, warn};

/// Type declaration for SendOperations
pub type SendOperationsStreamType = Pin<
    Box<
        dyn futures_util::Stream<Item = Result<grpc_api::SendOperationsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// This function takes a streaming request of operations messages,
/// verifies, saves and propagates the operations received in each message, and sends back a stream of
/// operations ids messages
pub(crate) async fn send_operations(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<tonic::Streaming<grpc_api::SendOperationsRequest>>,
) -> Result<SendOperationsStreamType, GrpcError> {
    let mut pool_controller = grpc.pool_controller.clone();
    let protocol_controller = grpc.protocol_controller.clone();
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
                            tx.clone(),
                            tonic::Code::InvalidArgument,
                            "the request payload is empty".to_owned(),
                        )
                        .await;
                    } else {
                        let now = MassaTime::now();
                        let Ok(last_slot) = get_latest_block_slot_at_timestamp(
                            config.thread_count,
                            config.t0,
                            config.genesis_timestamp,
                            now,
                        ) else {
                            report_error(
                                tx.clone(),
                                tonic::Code::InvalidArgument,
                                "failed to get current period".to_owned(),
                            )
                            .await;
                            continue;
                        };
                        // If there are too many operations in the incoming message, send an error message back to the client
                        if req_content.operations.len() as u32 > config.max_operations_per_message {
                            report_error(
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
                                    // Deserialize the operation and verify its signature
                                    let verified_op_res = match operation_deserializer.deserialize::<DeserializeError>(&proto_operation) {
                                        Ok(tuple) => {
                                            let (rest, res_operation): (&[u8], SecureShareOperation) = tuple;
                                            match res_operation.content.op {
                                                OperationType::CallSC { max_gas, .. } | OperationType::ExecuteSC { max_gas, .. } => {
                                                    if max_gas > config.max_gas_per_block {
                                                        return Err(GrpcError::InvalidArgument("Gas limit of the operation is higher than the block gas limit. Your operation will never be included in a block.".into()));
                                                    }
                                                },
                                                _ => {}
                                            };
                                            if let Some(slot) = last_slot {
                                                if res_operation.content.expire_period < slot.period {
                                                    return Err(GrpcError::InvalidArgument("Operation expire_period is lower than the current period of this node. Your operation will never be included in a block.".into()));
                                                }
                                            }
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
                                    pool_controller.add_operations(operation_storage.clone());

                                    // Propagate the operations to the network
                                    if let Err(e) =
                                        protocol_controller.propagate_operations(operation_storage)
                                    {
                                        // If propagation failed, send an error message back to the client
                                        let error =
                                            format!("failed to propagate operations: {}", e);
                                        report_error(
                                            tx.clone(),
                                            tonic::Code::Internal,
                                            error.to_owned(),
                                        )
                                        .await;
                                    };

                                    // Build the response message
                                    let result = grpc_model::OperationIds {
                                        operation_ids: verified_ops.keys().cloned().collect(),
                                    };
                                    // Send the response message back to the client
                                    if let Err(e) = tx
                                        .send(Ok(grpc_api::SendOperationsResponse {
                                            result: Some(
                                                grpc_api::send_operations_response::Result::OperationIds(
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
    Ok(Box::pin(out_stream) as SendOperationsStreamType)
}

// This function reports an error to the sender by sending a gRPC response message to the client
async fn report_error(
    sender: tokio::sync::mpsc::Sender<Result<grpc_api::SendOperationsResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) {
    error!("{}", error);
    // Attempt to send the error response message to the sender
    if let Err(e) = sender
        .send(Ok(grpc_api::SendOperationsResponse {
            result: Some(grpc_api::send_operations_response::Result::Error(
                grpc_model::Error {
                    code: code.into(),
                    message: error,
                },
            )),
        }))
        .await
    {
        // If sending the message fails, log the error message
        error!("failed to send back send_operations error response: {}", e);
    }
}
