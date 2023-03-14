use crate::error::{match_for_io_error, GrpcError};
use crate::handler::MassaGrpcService;
use futures_util::StreamExt;
use massa_models::endorsement::{EndorsementDeserializer, SecureShareEndorsement};
use massa_models::secure_share::SecureShareDeserializer;
use massa_proto::massa::api::v1::{self as grpc};
use massa_serialization::{DeserializeError, Deserializer};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::pin::Pin;
use tonic::codegen::futures_core;
use tracing::log::{error, warn};

/// type declaration for SendEndorsementsStream
pub type SendEndorsementsStream = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc::SendEndorsementsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

pub(crate) async fn send_endorsements(
    grpc: &MassaGrpcService,
    request: tonic::Request<tonic::Streaming<grpc::SendEndorsementsRequest>>,
) -> Result<SendEndorsementsStream, GrpcError> {
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
                    if req_content.endorsements.is_empty() {
                        send_endorsements_notify_error(
                            req_content.id.clone(),
                            tx.clone(),
                            tonic::Code::InvalidArgument,
                            "the request payload is empty".to_owned(),
                        )
                        .await;
                    } else {
                        let proto_endorsement = req_content.endorsements;
                        if proto_endorsement.len() as u32 > config.max_endorsements_per_message {
                            send_endorsements_notify_error(
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
                            let verified_eds_res: Result<HashMap<String, SecureShareEndorsement>, GrpcError> = proto_endorsement
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
                                                res_endorsement.verify_signature()
                                                    .map(|_| (res_endorsement.id.to_string(), res_endorsement))
                                                    .map_err(|e| e.into())
                                            } else {
                                                Err(GrpcError::InternalServerError(
                                                    "there is data left after endorsement deserialization".to_owned()
                                                ))
                                            }
                                        }
                                        Err(e) => {
                                            Err(GrpcError::InternalServerError(format!("failed to deserialize endorsement: {}", e)
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

                                    if let Err(e) =
                                        protocol_sender.propagate_endorsements(endorsement_storage)
                                    {
                                        let error =
                                            format!("failed to propagate endorsement: {}", e);
                                        send_endorsements_notify_error(
                                            req_content.id.clone(),
                                            tx.clone(),
                                            tonic::Code::Internal,
                                            error.to_owned(),
                                        )
                                        .await;
                                    };

                                    let result = grpc::EndorsementResult {
                                        ids: verified_eds.keys().cloned().collect(),
                                    };
                                    if let Err(e) = tx
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
                                        error!("failed to send back endorsement response: {}", e)
                                    };
                                }
                                Err(e) => {
                                    let error = format!("invalid endorsement(s): {}", e);
                                    send_endorsements_notify_error(
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

    Ok(Box::pin(out_stream) as SendEndorsementsStream)
}

async fn send_endorsements_notify_error(
    id: String,
    sender: tokio::sync::mpsc::Sender<Result<grpc::SendEndorsementsResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) {
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
}
