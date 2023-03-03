//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! gRPC API for a massa-node
use std::{collections::HashMap, error::Error, io::ErrorKind, pin::Pin};

use crate::config::GrpcConfig;
use massa_consensus_exports::{ConsensusChannels, ConsensusController};
use massa_models::{
    block::{BlockDeserializer, BlockDeserializerArgs, SecureShareBlock},
    error::ModelsError,
    operation::{OperationDeserializer, SecureShareOperation},
    secure_share::SecureShareDeserializer,
};
use massa_pool_exports::{PoolChannels, PoolController};
use massa_proto::massa::api::v1::{self as grpc, grpc_server::GrpcServer, FILE_DESCRIPTOR_SET};
use massa_serialization::{DeserializeError, Deserializer};

use futures_util::{FutureExt, StreamExt};
use massa_protocol_exports::ProtocolCommandSender;
use massa_storage::Storage;
use tokio::sync::oneshot;
use tonic::codegen::{futures_core, CompressionEncoding};
use tonic_web::GrpcWebLayer;
use tracing::log::{error, info, warn};

/// gRPC API content
pub struct MassaService {
    /// link(channels) to the consensus component
    pub consensus_controller: Box<dyn ConsensusController>,
    /// link(channels) to the consensus component
    pub consensus_channels: ConsensusChannels,
    /// link(channels) to the pool component
    pub pool_channels: PoolChannels,
    /// link to the pool component
    pub pool_command_sender: Box<dyn PoolController>,
    /// link(channels) to the protocol component
    pub protocol_command_sender: ProtocolCommandSender,
    /// link to the storage component
    pub storage: Storage,
    /// gRPC configuration
    pub grpc_config: GrpcConfig,
    /// node version
    pub version: massa_models::version::Version,
}

impl MassaService {
    /// Start the gRPC API
    pub async fn serve(
        self,
        config: &GrpcConfig,
    ) -> Result<StopHandle, Box<dyn std::error::Error>> {
        let mut svc = GrpcServer::new(self)
            .max_decoding_message_size(config.max_decoding_message_size)
            .max_encoding_message_size(config.max_encoding_message_size);

        if let Some(encoding) = &config.accept_compressed {
            if encoding.eq_ignore_ascii_case("Gzip") {
                svc = svc.accept_compressed(CompressionEncoding::Gzip);
            };
        }

        if let Some(encoding) = &config.accept_compressed {
            if encoding.eq_ignore_ascii_case("Gzip") {
                svc = svc.send_compressed(CompressionEncoding::Gzip);
            };
        }

        let (shutdown_send, shutdown_recv) = oneshot::channel::<()>();

        if config.accept_http1 {
            let mut router_with_http1 = tonic::transport::Server::builder()
                .accept_http1(true)
                .layer(GrpcWebLayer::new())
                .add_service(svc);

            if config.enable_reflection {
                let reflection_service = tonic_reflection::server::Builder::configure()
                    .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
                    .build()?;

                router_with_http1 = router_with_http1.add_service(reflection_service);
            }
            router_with_http1
                .serve_with_shutdown(config.bind, shutdown_recv.map(drop))
                .await?;
        } else {
            let mut router = tonic::transport::Server::builder().add_service(svc.clone());

            if config.enable_reflection {
                let reflection_service = tonic_reflection::server::Builder::configure()
                    .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
                    .build()?;

                router = router.add_service(reflection_service);
            }

            router
                .serve_with_shutdown(config.bind, shutdown_recv.map(drop))
                .await?;
        }

        Ok(StopHandle {
            server_handler: shutdown_send,
        })
    }
}

/// Used to be able to stop the gRPC API
pub struct StopHandle {
    server_handler: oneshot::Sender<()>,
}

impl StopHandle {
    /// stop the gRPC API gracefully
    pub fn stop(self) {
        match self.server_handler.send(()) {
            Ok(_) => {
                info!("gRPC API finished cleanly");
            }
            Err(err) => warn!("gRPC API thread panicked: {:?}", err),
        }
    }
}

fn match_for_io_error(err_status: &tonic::Status) -> Option<&std::io::Error> {
    let mut err: &(dyn Error + 'static) = err_status;

    loop {
        if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
            return Some(io_err);
        }

        // h2::Error do not expose std::io::Error with `source()`
        // https://github.com/hyperium/h2/pull/462
        if let Some(h2_err) = err.downcast_ref::<h2::Error>() {
            if let Some(io_err) = h2_err.get_io() {
                return Some(io_err);
            }
        }

        err = match err.source() {
            Some(err) => err,
            None => return None,
        };
    }
}

#[tonic::async_trait]
impl grpc::grpc_server::Grpc for MassaService {
    async fn get_version(
        &self,
        request: tonic::Request<grpc::GetVersionRequest>,
    ) -> Result<tonic::Response<grpc::GetVersionResponse>, tonic::Status> {
        Ok(tonic::Response::new(grpc::GetVersionResponse {
            id: request.into_inner().id,
            version: self.version.to_string(),
        }))
    }

    type SendBlocksStream = Pin<
        Box<
            dyn futures_core::Stream<Item = Result<grpc::SendBlocksResponse, tonic::Status>>
                + Send
                + 'static,
        >,
    >;

    async fn send_blocks(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::SendBlocksRequest>>,
    ) -> Result<tonic::Response<Self::SendBlocksStream>, tonic::Status> {
        let consensus_controller = self.consensus_controller.clone();
        let mut protocol_sender = self.protocol_command_sender.clone();
        let storage = self.storage.clone_without_refs();
        let config = self.grpc_config.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(config.max_channel_size);
        let mut in_stream = request.into_inner();

        tokio::spawn(async move {
            while let Some(result) = in_stream.next().await {
                match result {
                    Ok(req_content) => {
                        if let Some(proto_block) = req_content.block {
                            let args = BlockDeserializerArgs {
                                thread_count: config.thread_count,
                                max_operations_per_block: config.max_operations_per_block,
                                endorsement_count: config.endorsement_count,
                            };
                            let _res: Result<(), DeserializeError> =
                                match SecureShareDeserializer::new(BlockDeserializer::new(args))
                                    .deserialize::<DeserializeError>(
                                    &proto_block.serialized_content,
                                ) {
                                    Ok(tuple) => {
                                        let (rest, res_block): (&[u8], SecureShareBlock) = tuple;
                                        if rest.is_empty() {
                                            if let Ok(_verify_signature) = res_block
                                                .verify_signature()
                                                .and_then(|_| {
                                                    res_block.content.header.verify_signature()
                                                })
                                                .map(|_| {
                                                    res_block
                                                        .content
                                                        .header
                                                        .content
                                                        .endorsements
                                                        .iter()
                                                        .map(|endorsement| {
                                                            endorsement.verify_signature()
                                                        })
                                                        .collect::<Vec<Result<(), ModelsError>>>()
                                                })
                                            {
                                                let block_id = res_block.id;
                                                let slot = res_block.content.header.content.slot;
                                                let mut block_storage =
                                                    storage.clone_without_refs();
                                                block_storage.store_block(res_block.clone());
                                                consensus_controller.register_block(
                                                    block_id,
                                                    slot,
                                                    block_storage.clone(),
                                                    false,
                                                );
                                                let _res = match protocol_sender
                                                    .integrated_block(block_id, block_storage)
                                                {
                                                    Ok(()) => (),
                                                    Err(e) => {
                                                        let error = format!(
                                                            "failed to propagate block: {}",
                                                            e
                                                        );
                                                        let _res = sendblocks_notify_error(
                                                            req_content.id.clone(),
                                                            tx.clone(),
                                                            tonic::Code::Internal,
                                                            error.to_owned(),
                                                        )
                                                        .await;
                                                    }
                                                };
                                                let result = grpc::BlockResult {
                                                    id: res_block.id.to_string(),
                                                };
                                                let _res = match tx
                                                    .send(Ok(grpc::SendBlocksResponse {
                                                        id: req_content.id.clone(),
                                                        message: Some(grpc::send_blocks_response::Message::Result(result)),
                                                    }))
                                                    .await
                                                {
                                                    Ok(()) => (),
                                                    Err(e) => {
                                                        error!("failed to send back block response: {}", e)
                                                    }
                                                };
                                            } else {
                                                let error = format!(
                                                    "wrong signature: {}",
                                                    res_block.signature
                                                );
                                                let _res = sendblocks_notify_error(
                                                    req_content.id.clone(),
                                                    tx.clone(),
                                                    tonic::Code::InvalidArgument,
                                                    error.to_owned(),
                                                )
                                                .await;
                                            };
                                        } else {
                                            let _res = sendblocks_notify_error(
                                                req_content.id.clone(),
                                                tx.clone(),
                                                tonic::Code::InvalidArgument,
                                                "there is data left after operation deserialization".to_owned(),
                                            )
                                            .await;
                                        }
                                        Ok(())
                                    }
                                    Err(e) => {
                                        let error = format!("failed to deserialize block: {}", e);
                                        let _res = sendblocks_notify_error(
                                            req_content.id.clone(),
                                            tx.clone(),
                                            tonic::Code::InvalidArgument,
                                            error.to_owned(),
                                        )
                                        .await;
                                        Ok(())
                                    }
                                };
                        } else {
                            let _res = sendblocks_notify_error(
                                req_content.id.clone(),
                                tx.clone(),
                                tonic::Code::InvalidArgument,
                                "the request payload is empty".to_owned(),
                            )
                            .await;
                        }
                    }
                    Err(err) => {
                        if let Some(io_err) = match_for_io_error(&err) {
                            if io_err.kind() == ErrorKind::BrokenPipe {
                                warn!("client disconnected, broken pipe: {}", io_err);
                                break;
                            }
                        }
                        match tx.send(Err(err)).await {
                            Ok(_) => (),
                            Err(e) => {
                                error!("failed to send back sendblocks error response: {}", e);
                                break;
                            }
                        }
                    }
                }
            }
        });

        let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

        Ok(tonic::Response::new(
            Box::pin(out_stream) as Self::SendBlocksStream
        ))
    }

    type SendEndorsementsStream = Pin<
        Box<
            dyn futures_core::Stream<Item = Result<grpc::SendEndorsementsResponse, tonic::Status>>
                + Send
                + 'static,
        >,
    >;

    async fn send_endorsements(
        &self,
        _request: tonic::Request<tonic::Streaming<grpc::SendEndorsementsRequest>>,
    ) -> Result<tonic::Response<Self::SendEndorsementsStream>, tonic::Status> {
        Err(tonic::Status::unimplemented("not implemented"))
    }

    type SendOperationsStream = Pin<
        Box<
            dyn futures_core::Stream<Item = Result<grpc::SendOperationsResponse, tonic::Status>>
                + Send
                + 'static,
        >,
    >;

    async fn send_operations(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::SendOperationsRequest>>,
    ) -> Result<tonic::Response<Self::SendOperationsStream>, tonic::Status> {
        let mut cmd_sender = self.pool_command_sender.clone();
        let mut protocol_sender = self.protocol_command_sender.clone();
        let config = self.grpc_config.clone();
        let storage = self.storage.clone_without_refs();

        let (tx, rx) = tokio::sync::mpsc::channel(config.max_channel_size);
        let mut in_stream = request.into_inner();

        tokio::spawn(async move {
            while let Some(result) = in_stream.next().await {
                match result {
                    Ok(req_content) => {
                        if req_content.operations.is_empty() {
                            let _res = sendoperations_notify_error(
                                req_content.id.clone(),
                                tx.clone(),
                                tonic::Code::InvalidArgument,
                                "the request payload is empty".to_owned(),
                            )
                            .await;
                        } else {
                            let proto_operations = req_content.operations;
                            if proto_operations.len() as u32 > config.max_operations_per_message {
                                let _res = sendoperations_notify_error(
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
                                let verified_ops_res: Result<HashMap<String, SecureShareOperation>, ModelsError> = proto_operations
                                .into_iter()
                                .map(|proto_operation| {
                                    let mut op_serialized = Vec::new();
                                    op_serialized.extend(proto_operation.signature.as_bytes());
                                    op_serialized.extend(proto_operation.creator_public_key.as_bytes());
                                    op_serialized.extend(proto_operation.serialized_content);
                                    let verified_op = match operation_deserializer.deserialize::<DeserializeError>(&op_serialized) {
                                        Ok(tuple) => {
                                            let (rest, res_operation): (&[u8], SecureShareOperation) = tuple;
                                                if rest.is_empty() {
                                                if let Ok(_verify_signature) = res_operation.verify_signature() {
                                                        Ok((res_operation.id.to_string(), res_operation))
                                                    } else {
                                                        Err(ModelsError::MassaSignatureError(massa_signature::MassaSignatureError::SignatureError(
                                                            format!("wrong signature: {}", res_operation.signature).to_owned())
                                                        ))
                                                    }
                                                } else {
                                                    Err(ModelsError::DeserializeError(
                                                        "there is data left after operation deserialization".to_owned()
                                                    ))
                                                }
                                            },
                                        Err(e) => {
                                            Err(ModelsError::DeserializeError(format!("failed to deserialize operation: {}", e).to_owned()
                                            ))
                                        }
                                        };
                                        verified_op
                                })
                                .collect();

                                match verified_ops_res {
                                    Ok(verified_ops) => {
                                        let mut operation_storage = storage.clone_without_refs();
                                        operation_storage.store_operations(
                                            verified_ops.values().cloned().collect(),
                                        );
                                        cmd_sender.add_operations(operation_storage.clone());

                                        let _res = match protocol_sender
                                            .propagate_operations(operation_storage)
                                        {
                                            Ok(()) => (),
                                            Err(e) => {
                                                let error =
                                                    format!("failed to propagate operations: {}", e);
                                                let _res = sendoperations_notify_error(
                                                    req_content.id.clone(),
                                                    tx.clone(),
                                                    tonic::Code::Internal,
                                                    error.to_owned(),
                                                )
                                                .await;
                                            }
                                        };

                                        let result = grpc::OperationResult {
                                            ids: verified_ops.keys().cloned().collect(),
                                        };
                                        let _res = match tx
                                            .send(Ok(grpc::SendOperationsResponse {
                                                id: req_content.id.clone(),
                                                message: Some(
                                                    grpc::send_operations_response::Message::Result(
                                                        result,
                                                    ),
                                                ),
                                            }))
                                            .await
                                        {
                                            Ok(()) => (),
                                            Err(e) => {
                                                error!(
                                                    "failed to send back operations response: {}",
                                                    e
                                                )
                                            }
                                        };
                                    }
                                    Err(e) => {
                                        let error = format!("invalid operations:{}", e);
                                        let _res = sendoperations_notify_error(
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
                        match tx.send(Err(err)).await {
                            Ok(_) => (),
                            Err(e) => {
                                error!("failed to send back sendblocks error response: {}", e);
                                break;
                            }
                        }
                    }
                }
            }
        });

        let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

        Ok(tonic::Response::new(
            Box::pin(out_stream) as Self::SendOperationsStream
        ))
    }
}

async fn sendblocks_notify_error(
    id: String,
    sender: tokio::sync::mpsc::Sender<Result<grpc::SendBlocksResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) -> Result<(), Box<dyn Error>> {
    error!("{}", error);
    match sender
        .send(Ok(grpc::SendBlocksResponse {
            id,
            message: Some(grpc::send_blocks_response::Message::Error(
                massa_proto::google::rpc::Status {
                    code: code.into(),
                    message: error,
                    details: Vec::new(),
                },
            )),
        }))
        .await
    {
        Ok(()) => Ok(()),
        Err(e) => {
            error!("failed to send back sendblocks error response: {}", e);
            Ok(())
        }
    }
}

async fn sendoperations_notify_error(
    id: String,
    sender: tokio::sync::mpsc::Sender<Result<grpc::SendOperationsResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) -> Result<(), Box<dyn Error>> {
    error!("{}", error);
    match sender
        .send(Ok(grpc::SendOperationsResponse {
            id,
            message: Some(grpc::send_operations_response::Message::Error(
                massa_proto::google::rpc::Status {
                    code: code.into(),
                    message: error,
                    details: Vec::new(),
                },
            )),
        }))
        .await
    {
        Ok(()) => Ok(()),
        Err(e) => {
            error!("failed to send back sendoperations error response: {}", e);
            Ok(())
        }
    }
}
