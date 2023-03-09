use crate::api::MassaGrpcService;
use crate::error::{match_for_io_error, GrpcError};
use crate::models::SendBlocksStream;
use futures_util::StreamExt;
use itertools::izip;
use massa_models::address::Address;
use massa_models::block::{BlockDeserializer, BlockDeserializerArgs, SecureShareBlock};
use massa_models::error::ModelsError;
use massa_models::secure_share::SecureShareDeserializer;
use massa_models::slot::Slot;
use massa_models::timeslots;
use massa_proto::google::rpc::Status;
use massa_proto::massa::api::v1::{self as grpc, GetSelectorDrawsResponse, SendBlocksResponse};
use massa_proto::massa::api::v1::{GetDatastoreEntriesResponse, GetVersionResponse};
use massa_serialization::{DeserializeError, Deserializer};
use std::io::ErrorKind;
use std::str::FromStr;
use tokio::sync::mpsc::Sender;
use tonic::Request;
use tracing::log::{error, warn};

/// get version
pub(crate) fn get_version(
    grpc: &MassaGrpcService,
    request: Request<grpc::GetVersionRequest>,
) -> Result<GetVersionResponse, GrpcError> {
    Ok(GetVersionResponse {
        id: request.into_inner().id,
        version: grpc.version.to_string(),
    })
}

/// Get multiple datastore entries.
pub(crate) fn get_datastore_entries(
    grpc: &MassaGrpcService,
    request: Request<grpc::GetDatastoreEntriesRequest>,
) -> Result<GetDatastoreEntriesResponse, GrpcError> {
    let execution_controller = grpc.execution_controller.clone();
    let inner_req = request.into_inner();
    let id = inner_req.id.clone();

    let filters = inner_req
        .queries
        .into_iter()
        .map(|query| {
            let filter = query.filter.unwrap();
            Address::from_str(filter.address.as_str()).map(|address| (address, filter.key))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let entries = execution_controller
        .get_final_and_active_data_entry(filters)
        .into_iter()
        .map(|output| grpc::BytesMapFieldEntry {
            //TODO this behaviour should be confirmed
            key: output.0.unwrap_or_default(),
            value: output.1.unwrap_or_default(),
        })
        .collect();

    Ok(GetDatastoreEntriesResponse { id, entries })
}

pub(crate) fn get_selector_draws(
    grpc: &MassaGrpcService,
    request: Request<grpc::GetSelectorDrawsRequest>,
) -> Result<GetSelectorDrawsResponse, GrpcError> {
    let selector_controller = grpc.selector_controller.clone();
    let config = grpc.grpc_config.clone();
    let inner_req = request.into_inner();
    let id = inner_req.id.clone();

    let addresses = inner_req
        .queries
        .into_iter()
        .map(|query| Address::from_str(query.filter.unwrap().address.as_str()))
        .collect::<Result<Vec<_>, _>>()?;

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
                let mut proto_nt_endorsement_draws = Vec::with_capacity(addresses.len());
                let iterator = izip!(nt_block_draws.into_iter(), nt_endorsement_draws.into_iter());
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

    Ok(GetSelectorDrawsResponse {
        id,
        selector_draws: res,
    })
}

// ███████╗████████╗██████╗ ███████╗ █████╗ ███╗   ███╗
// ██╔════╝╚══██╔══╝██╔══██╗██╔════╝██╔══██╗████╗ ████║
// ███████╗   ██║   ██████╔╝█████╗  ███████║██╔████╔██║
// ╚════██║   ██║   ██╔══██╗██╔══╝  ██╔══██║██║╚██╔╝██║
// ███████║   ██║   ██║  ██║███████╗██║  ██║██║ ╚═╝ ██║
// ╚══════╝   ╚═╝   ╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝╚═╝     ╚═╝

pub(crate) async fn send_blocks(
    grpc: &MassaGrpcService,
    request: Request<tonic::Streaming<grpc::SendBlocksRequest>>,
) -> Result<SendBlocksStream, GrpcError> {
    let consensus_controller = grpc.consensus_controller.clone();
    let mut protocol_sender = grpc.protocol_command_sender.clone();
    let storage = grpc.storage.clone_without_refs();
    let config = grpc.grpc_config.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(config.max_channel_size);
    let mut in_stream = request.into_inner();

    tokio::spawn(async move {
        while let Some(result) = in_stream.next().await {
            let sender = tx.clone();
            match result {
                Ok(req_content) => {
                    let Some(proto_block) = req_content.block else {
                        send_blocks_notify_error(
                            req_content.id.clone(),
                            sender,
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

                    let _: Result<(), DeserializeError> =
                        match SecureShareDeserializer::new(BlockDeserializer::new(args))
                            .deserialize::<DeserializeError>(&proto_block.serialized_content)
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
                                        // Signature error
                                        send_blocks_notify_error(
                                            req_content.id.clone(),
                                            sender,
                                            tonic::Code::InvalidArgument,
                                            format!("wrong signature: {}", e),
                                        )
                                        .await;
                                        continue;
                                    }

                                    let block_id = res_block.id;
                                    let slot = res_block.content.header.content.slot;
                                    let mut block_storage = storage.clone_without_refs();
                                    block_storage.store_block(res_block.clone());
                                    consensus_controller.register_block(
                                        block_id,
                                        slot,
                                        block_storage.clone(),
                                        false,
                                    );

                                    if let Err(e) =
                                        protocol_sender.integrated_block(block_id, block_storage)
                                    {
                                        send_blocks_notify_error(
                                            req_content.id.clone(),
                                            sender,
                                            tonic::Code::Internal,
                                            format!("failed to propagate block: {}", e),
                                        )
                                        .await;
                                        // continue ?
                                        continue;
                                    };

                                    let result = grpc::BlockResult {
                                        id: res_block.id.to_string(),
                                    };

                                    if let Err(e) = tx
                                        .send(Ok(SendBlocksResponse {
                                            id: req_content.id.clone(),
                                            message: Some(
                                                grpc::send_blocks_response::Message::Result(result),
                                            ),
                                        }))
                                        .await
                                    {
                                        error!("failed to send back block response: {}", e);
                                    };
                                } else {
                                    send_blocks_notify_error(
                                        req_content.id.clone(),
                                        sender,
                                        tonic::Code::InvalidArgument,
                                        "there is data left after operation deserialization"
                                            .to_owned(),
                                    )
                                    .await;
                                }
                                Ok(())
                            }
                            Err(e) => {
                                send_blocks_notify_error(
                                    req_content.id.clone(),
                                    sender,
                                    tonic::Code::InvalidArgument,
                                    format!("failed to deserialize block: {}", e),
                                )
                                .await;
                                Ok(())
                            }
                        };
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

    Ok(Box::pin(out_stream) as SendBlocksStream)
}

async fn send_blocks_notify_error(
    id: String,
    sender: Sender<Result<SendBlocksResponse, tonic::Status>>,
    code: tonic::Code,
    error: String,
) {
    error!("{}", error);
    if let Err(e) = sender
        .send(Ok(SendBlocksResponse {
            id,
            message: Some(grpc::send_blocks_response::Message::Error(Status {
                code: code.into(),
                message: error,
                details: Vec::new(),
            })),
        }))
        .await
    {
        error!("failed to send back send_blocks error response: {}", e);
    }
}
