// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::{match_for_io_error, GrpcError};
use crate::server::MassaPublicGrpc;
use futures_util::StreamExt;
use massa_models::endorsement::{EndorsementDeserializer, SecureShareEndorsement};
use massa_models::mapping_grpc::secure_share_to_vec;
use massa_models::secure_share::SecureShareDeserializer;
use massa_proto_rs::massa::api::v1 as grpc_api;
use massa_proto_rs::massa::model::v1 as grpc_model;
use massa_serialization::{DeserializeError, Deserializer};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::pin::Pin;
use tonic::codegen::futures_core;
use tracing::log::{error, warn};

/// Type declaration for SendEndorsements
pub type SendEndorsementsStreamType = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc_api::SendEndorsementsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// This function takes a streaming request of endorsements messages,
/// verifies, saves and propagates the endorsements received in each message, and sends back a stream of
/// endorsements ids messages
pub(crate) async fn send_endorsements(
    grpc: &MassaPublicGrpc,
    request: tonic::Request<tonic::Streaming<grpc_api::SendEndorsementsRequest>>,
) -> Result<SendEndorsementsStreamType, GrpcError> {
    let mut pool_command_sender = grpc.pool_controller.clone();
    let protocol_command_sender = grpc.protocol_controller.clone();
    let config = grpc.grpc_config.clone();
    let storage = grpc.storage.clone_without_refs();

    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(config.max_channel_size);
    // Extract the incoming stream of endorsements messages
    let mut in_stream = request.into_inner();

    // Spawn a task that reads incoming messages and processes the endorsements in each message
    tokio::spawn(async move {
        while let Some(result) = in_stream.next().await {
            match result {
                Ok(req_content) => {
                    // If the incoming message has no endorsements, send an error message back to the client
                    if req_content.endorsements.is_empty() {
                        report_error(
                            tx.clone(),
                            tonic::Code::InvalidArgument,
                            "the request payload is empty".to_owned(),
                        )
                        .await;
                    } else {
                        // If there are too many endorsements in the incoming message, send an error message back to the client
                        let proto_endorsement = req_content.endorsements;
                        if proto_endorsement.len() as u32 > config.max_endorsements_per_message {
                            report_error(
                                tx.clone(),
                                tonic::Code::InvalidArgument,
                                "too many endorsements per message".to_owned(),
                            )
                            .await;
                        } else {
                            // Deserialize and verify each endorsement in the incoming message
                            let endorsement_deserializer =
                                SecureShareDeserializer::new(EndorsementDeserializer::new(
                                    config.thread_count,
                                    config.endorsement_count,
                                ));
                            let verified_eds_res: Result<HashMap<String, SecureShareEndorsement>, GrpcError> = proto_endorsement
                                .into_iter()
                                .map(|proto_endorsement| {
                                    let ed_serialized = secure_share_to_vec(proto_endorsement)?;
                                    let verified_op = match endorsement_deserializer.deserialize::<DeserializeError>(&ed_serialized) {
                                        Ok(tuple) => {
                                            // Deserialize the endorsement and verify its signature
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
                                // If all endorsements in the incoming message are valid, store and propagate them
                                Ok(verified_eds) => {
                                    let mut endorsement_storage = storage.clone_without_refs();
                                    endorsement_storage.store_endorsements(
                                        verified_eds.values().cloned().collect(),
                                    );
                                    // Add the received endorsements to the endorsements pool
                                    pool_command_sender
                                        .add_endorsements(endorsement_storage.clone());

                                    // Propagate the endorsements to the network
                                    if let Err(e) = protocol_command_sender
                                        .propagate_endorsements(endorsement_storage)
                                    {
                                        // If propagation failed, send an error message back to the client
                                        let error =
                                            format!("failed to propagate endorsement: {}", e);
                                        report_error(
                                            tx.clone(),
                                            tonic::Code::Internal,
                                            error.to_owned(),
                                        )
                                        .await;
                                    };

                                    // Build the response message
                                    let result = grpc_model::EndorsementIds {
                                        endorsements_ids: verified_eds.keys().cloned().collect(),
                                    };
                                    // Send the response message back to the client
                                    if let Err(e) = tx
                                        .send(Ok(grpc_api::SendEndorsementsResponse {
                                            result: Some(
                                                grpc_api::send_endorsements_response::Result::EndorsementIds(
                                                    result,
                                                ),
                                            ),
                                        }))
                                        .await
                                    {
                                        error!("failed to send back endorsement response: {}", e)
                                    };
                                }
                                // If the verification failed, send an error message back to the client
                                Err(e) => {
                                    let error = format!("invalid endorsement(s): {}", e);
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
                // Handles errors that occur while sending a response back to a client
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
    Ok(Box::pin(out_stream) as SendEndorsementsStreamType)
}

// This function reports an error to the sender by sending a gRPC response message to the client
async fn report_error(
    sender: tokio::sync::mpsc::Sender<Result<grpc_api::SendEndorsementsResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) {
    error!("{}", error);
    // Attempt to send the error response message to the sender
    if let Err(e) = sender
        .send(Ok(grpc_api::SendEndorsementsResponse {
            result: Some(grpc_api::send_endorsements_response::Result::Error(
                massa_proto_rs::massa::model::v1::Error {
                    code: code.into(),
                    message: error,
                },
            )),
        }))
        .await
    {
        // If sending the message fails, log the error message
        error!(
            "failed to send back send_endorsements error response: {}",
            e
        );
    }
}
