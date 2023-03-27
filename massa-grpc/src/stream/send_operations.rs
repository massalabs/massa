use crate::error::{match_for_io_error, GrpcError};
use crate::service::MassaGrpcService;
use futures_util::StreamExt;
use massa_models::operation::{OperationDeserializer, SecureShareOperation};
use massa_models::secure_share::SecureShareDeserializer;
use massa_proto::massa::api::v1::{self as grpc};
use massa_serialization::{DeserializeError, Deserializer};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::pin::Pin;
use tonic::codegen::futures_core;
use tracing::log::{error, warn};

/// type declaration for SendOperationsStream
pub type SendOperationsStream = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc::SendOperationsStreamResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

pub(crate) async fn send_operations(
    grpc: &MassaGrpcService,
    request: tonic::Request<tonic::Streaming<grpc::SendOperationsStreamRequest>>,
) -> Result<SendOperationsStream, GrpcError> {
    let mut cmd_sender = grpc.pool_command_sender.clone();
    let mut protocol_sender = grpc.protocol_command_sender.clone();
    let config = grpc.grpc_config.clone();
    let storage = grpc.storage.clone_without_refs();

    let (tx, rx) = tokio::sync::mpsc::channel(config.max_channel_size);
    let mut in_stream = request.into_inner();

    tokio::spawn(async move {
        while let Some(result) = in_stream.next().await {
            match result {
                Ok(req_content) => {
                    if req_content.operations.is_empty() {
                        send_operations_notify_error(
                            req_content.id.clone(),
                            tx.clone(),
                            tonic::Code::InvalidArgument,
                            "the request payload is empty".to_owned(),
                        )
                        .await;
                    } else {
                        let proto_operations = req_content.operations;
                        if proto_operations.len() as u32 > config.max_operations_per_message {
                            send_operations_notify_error(
                                req_content.id.clone(),
                                tx.clone(),
                                tonic::Code::InvalidArgument,
                                "too many operations".to_owned(),
                            )
                            .await;
                        } else {
                            let operation_deserializer =
                                SecureShareDeserializer::new(OperationDeserializer::new(
                                    config.max_datastore_value_length,
                                    config.max_function_name_length,
                                    config.max_parameter_size,
                                    config.max_op_datastore_entry_count,
                                    config.max_op_datastore_key_length,
                                    config.max_op_datastore_value_length,
                                ));
                            let verified_ops_res: Result<HashMap<String, SecureShareOperation>, GrpcError> = proto_operations
                                .into_iter()
                                .map(|proto_operation| {
                                    let mut op_serialized = Vec::new();
                                    op_serialized.extend(proto_operation.signature.as_bytes());
                                    op_serialized.extend(proto_operation.content_creator_pub_key.as_bytes());
                                    op_serialized.extend(proto_operation.serialized_data);
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
                                Ok(verified_ops) => {
                                    let mut operation_storage = storage.clone_without_refs();
                                    operation_storage
                                        .store_operations(verified_ops.values().cloned().collect());
                                    cmd_sender.add_operations(operation_storage.clone());

                                    if let Err(e) =
                                        protocol_sender.propagate_operations(operation_storage)
                                    {
                                        let error =
                                            format!("failed to propagate operations: {}", e);
                                        send_operations_notify_error(
                                            req_content.id.clone(),
                                            tx.clone(),
                                            tonic::Code::Internal,
                                            error.to_owned(),
                                        )
                                        .await;
                                    };

                                    let result = grpc::OperationResult {
                                        operations_ids: verified_ops.keys().cloned().collect(),
                                    };
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
                                Err(e) => {
                                    let error = format!("invalid operation(s): {}", e);
                                    send_operations_notify_error(
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
                Err(err) => {
                    if let Some(io_err) = match_for_io_error(&err) {
                        if io_err.kind() == ErrorKind::BrokenPipe {
                            warn!("client disconnected, broken pipe: {}", io_err);
                            break;
                        }
                    }
                    error!("{}", err);
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

async fn send_operations_notify_error(
    id: String,
    sender: tokio::sync::mpsc::Sender<Result<grpc::SendOperationsStreamResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) {
    error!("{}", error);
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
        error!("failed to send back send_operations error response: {}", e);
    }
}
