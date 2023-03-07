//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! gRPC API for a massa-node
use std::{collections::HashMap, error::Error, io::ErrorKind, pin::Pin, str::FromStr};

use crate::{config::GrpcConfig, error::match_for_io_error};
use itertools::izip;
use massa_consensus_exports::{ConsensusChannels, ConsensusController};
use massa_execution_exports::ExecutionController;
use massa_models::{
    address::Address,
    block::{BlockDeserializer, BlockDeserializerArgs, SecureShareBlock},
    endorsement::{EndorsementDeserializer, SecureShareEndorsement},
    error::ModelsError,
    operation::{OperationDeserializer, SecureShareOperation},
    secure_share::SecureShareDeserializer,
    slot::Slot,
    timeslots,
};
use massa_pool_exports::{PoolChannels, PoolController};
use massa_pos_exports::SelectorController;
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
pub struct MassaGrpcService {
    /// link(channels) to the consensus component
    pub consensus_controller: Box<dyn ConsensusController>,
    /// link(channels) to the consensus component
    pub consensus_channels: ConsensusChannels,
    /// link to the execution component
    pub execution_controller: Box<dyn ExecutionController>,
    /// link(channels) to the pool component
    pub pool_channels: PoolChannels,
    /// link to the pool component
    pub pool_command_sender: Box<dyn PoolController>,
    /// link(channels) to the protocol component
    pub protocol_command_sender: ProtocolCommandSender,
    /// link to the selector component
    pub selector_controller: Box<dyn SelectorController>,
    /// link to the storage component
    pub storage: Storage,
    /// gRPC configuration
    pub grpc_config: GrpcConfig,
    /// node version
    pub version: massa_models::version::Version,
}

impl MassaGrpcService {
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

        if let Some(encoding) = &config.send_compressed {
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
            stop_cmd_sender: shutdown_send,
        })
    }
}

/// Used to be able to stop the gRPC API
pub struct StopHandle {
    stop_cmd_sender: oneshot::Sender<()>,
}

impl StopHandle {
    /// stop the gRPC API gracefully
    pub fn stop(self) {
        if let Err(e) = self.stop_cmd_sender.send(()) {
            warn!("gRPC API thread panicked: {:?}", e);
        } else {
            info!("gRPC API finished cleanly");
        }
    }
}

#[tonic::async_trait]
impl grpc::grpc_server::Grpc for MassaGrpcService {
    async fn get_selector_draws(
        &self,
        request: tonic::Request<grpc::GetSelectorDrawsRequest>,
    ) -> Result<tonic::Response<grpc::GetSelectorDrawsResponse>, tonic::Status> {
        let selector_controller = self.selector_controller.clone();
        let config = self.grpc_config.clone();
        let inner_req = request.into_inner();
        let id = inner_req.id.clone();

        let addresses_res = inner_req
            .queries
            .into_iter()
            .map(|query| {
                //TODO remove unwrap ?
                Address::from_str(query.filter.unwrap().address.as_str())
            })
            .collect::<Result<Vec<_>, _>>();

        match addresses_res {
            Ok(addresses) => {
                // get future draws from selector
                let selection_draws = {
                    let cur_slot = timeslots::get_current_latest_block_slot(
                        config.thread_count,
                        config.t0,
                        config.genesis_timestamp,
                    )
                    .expect("could not get latest current slot")
                    .unwrap_or_else(|| Slot::new(0, 0));
                    let slot_end = Slot::new(
                        cur_slot
                            .period
                            .saturating_add(config.draw_lookahead_period_count),
                        cur_slot.thread,
                    );
                    addresses
                        .iter()
                        .map(|addr| {
                            let (nt_block_draws, nt_endorsement_draws) = selector_controller
                                .get_address_selections(addr, cur_slot, slot_end)
                                .unwrap_or_default();

                            let mut proto_nt_block_draws = Vec::with_capacity(addresses.len());
                            let mut proto_nt_endorsement_draws =
                                Vec::with_capacity(addresses.len());
                            let iterator =
                                izip!(nt_block_draws.into_iter(), nt_endorsement_draws.into_iter());
                            for (next_block_draw, next_endorsement_draw) in iterator {
                                proto_nt_block_draws.push(next_block_draw.into());
                                proto_nt_endorsement_draws.push(next_endorsement_draw.into());
                            }

                            (proto_nt_block_draws, proto_nt_endorsement_draws)
                        })
                        .collect::<Vec<_>>()
                };

                // compile results
                let mut res = Vec::with_capacity(addresses.len());
                let iterator = izip!(addresses.into_iter(), selection_draws.into_iter());
                for (address, (next_block_draws, next_endorsement_draws)) in iterator {
                    res.push(grpc::SelectorDraws {
                        address: address.to_string(),
                        next_block_draws,
                        next_endorsement_draws,
                    });
                }

                Ok(tonic::Response::new(grpc::GetSelectorDrawsResponse {
                    id,
                    selector_draws: res,
                }))
            }

            Err(e) => {
                return Err(tonic::Status::invalid_argument(e.to_string()));
            }
        }
    }

    /// Get multiple datastore entries.
    async fn get_datastore_entries(
        &self,
        request: tonic::Request<grpc::GetDatastoreEntriesRequest>,
    ) -> Result<tonic::Response<grpc::GetDatastoreEntriesResponse>, tonic::Status> {
        let execution_controller = self.execution_controller.clone();
        let inner_req = request.into_inner();
        let id = inner_req.id.clone();

        let filters = inner_req
            .entries
            .into_iter()
            .map(|filter| {
                Address::from_str(filter.address.as_str()).map(|address| (address, filter.key))
            })
            .collect::<Result<Vec<_>, _>>();

        match filters {
            Ok(filters) => {
                let entries = execution_controller
                    .get_final_and_active_data_entry(filters)
                    .into_iter()
                    .map(|output| grpc::BytesMapFieldEntry {
                        //TODO this behaviour should be confirmed
                        key: output.0.unwrap_or_default(),
                        value: output.1.unwrap_or_default(),
                    })
                    .collect();

                Ok(tonic::Response::new(grpc::GetDatastoreEntriesResponse {
                    id,
                    entries,
                }))
            }

            Err(e) => {
                return Err(tonic::Status::invalid_argument(e.to_string()));
            }
        }
    }

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
                            let _: Result<(), DeserializeError> =
                                match SecureShareDeserializer::new(BlockDeserializer::new(args))
                                    .deserialize::<DeserializeError>(
                                    &proto_block.serialized_content,
                                ) {
                                    Ok(tuple) => {
                                        let (rest, res_block): (&[u8], SecureShareBlock) = tuple;
                                        if rest.is_empty() {
                                            if let Ok(_) = res_block
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
                                                let _ = match protocol_sender
                                                    .integrated_block(block_id, block_storage)
                                                {
                                                    Ok(()) => (),
                                                    Err(e) => {
                                                        let error = format!(
                                                            "failed to propagate block: {}",
                                                            e
                                                        );
                                                        let _ = send_blocks_notify_error(
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

                                                if let Err(e) = tx
                                                    .send(Ok(grpc::SendBlocksResponse {
                                                        id: req_content.id.clone(),
                                                        message: Some(grpc::send_blocks_response::Message::Result(result)),
                                                    }))
                                                    .await
                                                {
                                                        error!("failed to send back block response: {}", e)
                                                };
                                            } else {
                                                let error = format!(
                                                    "wrong signature: {}",
                                                    res_block.signature
                                                );
                                                let _ = send_blocks_notify_error(
                                                    req_content.id.clone(),
                                                    tx.clone(),
                                                    tonic::Code::InvalidArgument,
                                                    error.to_owned(),
                                                )
                                                .await;
                                            };
                                        } else {
                                            let _ = send_blocks_notify_error(
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
                                        let _ = send_blocks_notify_error(
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
                            let _ = send_blocks_notify_error(
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
                        error!("{}", err);
                        if let Err(e) = tx.send(Err(err)).await {
                            error!("failed to send back send_blocks error response: {}", e);
                            break;
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
        request: tonic::Request<tonic::Streaming<grpc::SendEndorsementsRequest>>,
    ) -> Result<tonic::Response<Self::SendEndorsementsStream>, tonic::Status> {
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
                        if req_content.endorsements.is_empty() {
                            let _ = send_endorsements_notify_error(
                                req_content.id.clone(),
                                tx.clone(),
                                tonic::Code::InvalidArgument,
                                "the request payload is empty".to_owned(),
                            )
                            .await;
                        } else {
                            let proto_endorsement = req_content.endorsements;
                            if proto_endorsement.len() as u32 > config.max_endorsements_per_message
                            {
                                let _ = send_endorsements_notify_error(
                                    req_content.id.clone(),
                                    tx.clone(),
                                    tonic::Code::InvalidArgument,
                                    "too many endorsements".to_owned(),
                                )
                                .await;
                            } else {
                                let endorsement_deserializer =
                                    SecureShareDeserializer::new(EndorsementDeserializer::new(
                                        config.thread_count,
                                        config.endorsement_count,
                                    ));
                                let verified_eds_res: Result<HashMap<String, SecureShareEndorsement>, ModelsError> = proto_endorsement
                                .into_iter()
                                .map(|proto_endorsement| {
                                    let mut ed_serialized = Vec::new();
                                    ed_serialized.extend(proto_endorsement.signature.as_bytes());
                                    ed_serialized.extend(proto_endorsement.creator_public_key.as_bytes());
                                    ed_serialized.extend(proto_endorsement.serialized_content);
                                    let verified_op = match endorsement_deserializer.deserialize::<DeserializeError>(&ed_serialized) {
                                        Ok(tuple) => {
                                            let (rest, res_endorsement): (&[u8], SecureShareEndorsement) = tuple;
                                                if rest.is_empty() {
                                                if let Ok(_) = res_endorsement.verify_signature() {
                                                        Ok((res_endorsement.id.to_string(), res_endorsement))
                                                    } else {
                                                        Err(ModelsError::MassaSignatureError(massa_signature::MassaSignatureError::SignatureError(
                                                            format!("wrong signature: {}", res_endorsement.signature))
                                                        ))
                                                    }
                                                } else {
                                                    Err(ModelsError::DeserializeError(
                                                        "there is data left after endorsement deserialization".to_owned()
                                                    ))
                                                }
                                            },
                                        Err(e) => {
                                            Err(ModelsError::DeserializeError(format!("failed to deserialize endorsement: {}", e)
                                            ))
                                        }
                                        };
                                        verified_op
                                })
                                .collect();

                                match verified_eds_res {
                                    Ok(verified_eds) => {
                                        let mut endorsement_storage = storage.clone_without_refs();
                                        endorsement_storage.store_endorsements(
                                            verified_eds.values().cloned().collect(),
                                        );
                                        cmd_sender.add_endorsements(endorsement_storage.clone());

                                        let _ = match protocol_sender
                                            .propagate_endorsements(endorsement_storage)
                                        {
                                            Ok(()) => (),
                                            Err(e) => {
                                                let error = format!(
                                                    "failed to propagate endorsement: {}",
                                                    e
                                                );
                                                let _ = send_endorsements_notify_error(
                                                    req_content.id.clone(),
                                                    tx.clone(),
                                                    tonic::Code::Internal,
                                                    error.to_owned(),
                                                )
                                                .await;
                                            }
                                        };

                                        let result = grpc::EndorsementResult {
                                            ids: verified_eds.keys().cloned().collect(),
                                        };
                                        if let Err(e)  = tx
                                            .send(Ok(grpc::SendEndorsementsResponse {
                                                id: req_content.id.clone(),
                                                message: Some(
                                                    grpc::send_endorsements_response::Message::Result(
                                                        result,
                                                    ),
                                                ),
                                            }))
                                            .await
                                        {
                                                error!(
                                                    "failed to send back endorsement response: {}",
                                                    e
                                                )
                                        };
                                    }
                                    Err(e) => {
                                        let error = format!("invalid endorsement(s): {}", e);
                                        let _ = send_endorsements_notify_error(
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
                            error!(
                                "failed to send back send_endorsements error response: {}",
                                e
                            );
                            break;
                        }
                    }
                }
            }
        });

        let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

        Ok(tonic::Response::new(
            Box::pin(out_stream) as Self::SendEndorsementsStream
        ))
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
                            let _ = send_operations_notify_error(
                                req_content.id.clone(),
                                tx.clone(),
                                tonic::Code::InvalidArgument,
                                "the request payload is empty".to_owned(),
                            )
                            .await;
                        } else {
                            let proto_operations = req_content.operations;
                            if proto_operations.len() as u32 > config.max_operations_per_message {
                                let _ = send_operations_notify_error(
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
                                                if let Ok(_) = res_operation.verify_signature() {
                                                        Ok((res_operation.id.to_string(), res_operation))
                                                    } else {
                                                        Err(ModelsError::MassaSignatureError(massa_signature::MassaSignatureError::SignatureError(
                                                            format!("wrong signature: {}", res_operation.signature))
                                                        ))
                                                    }
                                                } else {
                                                    Err(ModelsError::DeserializeError(
                                                        "there is data left after operation deserialization".to_owned()
                                                    ))
                                                }
                                            },
                                        Err(e) => {
                                            Err(ModelsError::DeserializeError(format!("failed to deserialize operation: {}", e)))
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

                                        let _ = match protocol_sender
                                            .propagate_operations(operation_storage)
                                        {
                                            Ok(()) => (),
                                            Err(e) => {
                                                let error = format!(
                                                    "failed to propagate operations: {}",
                                                    e
                                                );
                                                let _ = send_operations_notify_error(
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
                                        if let Err(e) = tx
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
                                            error!("failed to send back operations response: {}", e)
                                        };
                                    }
                                    Err(e) => {
                                        let error = format!("invalid operation(s): {}", e);
                                        let _ = send_operations_notify_error(
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

        Ok(tonic::Response::new(
            Box::pin(out_stream) as Self::SendOperationsStream
        ))
    }
}

async fn send_blocks_notify_error(
    id: String,
    sender: tokio::sync::mpsc::Sender<Result<grpc::SendBlocksResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) -> Result<(), Box<dyn Error>> {
    error!("{}", error);
    if let Err(e) = sender
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
        error!("failed to send back send_blocks error response: {}", e);
    }

    Ok(())
}

async fn send_endorsements_notify_error(
    id: String,
    sender: tokio::sync::mpsc::Sender<Result<grpc::SendEndorsementsResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) -> Result<(), Box<dyn Error>> {
    error!("{}", error);
    if let Err(e) = sender
        .send(Ok(grpc::SendEndorsementsResponse {
            id,
            message: Some(grpc::send_endorsements_response::Message::Error(
                massa_proto::google::rpc::Status {
                    code: code.into(),
                    message: error,
                    details: Vec::new(),
                },
            )),
        }))
        .await
    {
        error!(
            "failed to send back send_endorsements error response: {}",
            e
        );
    }

    Ok(())
}

async fn send_operations_notify_error(
    id: String,
    sender: tokio::sync::mpsc::Sender<Result<grpc::SendOperationsResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) -> Result<(), Box<dyn Error>> {
    error!("{}", error);
    if let Err(e) = sender
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
        error!("failed to send back send_operations error response: {}", e);
    }

    Ok(())
}
