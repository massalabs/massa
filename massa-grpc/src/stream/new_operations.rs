use crate::error::GrpcError;
use crate::service::MassaGrpcService;
use futures_util::StreamExt;
use massa_models::operation::OperationType;
use massa_proto::massa::api::v1 as grpc;
use std::pin::Pin;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::error;

/// Type declaration for StreamTransactionsThroughputStream
pub type NewOperationsStream = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc::NewOperationsStreamResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Creates a new stream of new produced and received operations
pub(crate) async fn new_operations(
    grpc: &MassaGrpcService,
    request: Request<Streaming<grpc::NewOperationsStreamRequest>>,
) -> Result<NewOperationsStream, GrpcError> {
    // Create a channel to handle communication with the client
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Get the inner stream from the request
    let mut in_stream = request.into_inner();
    // Subscribe to the new operations channel
    let mut subscriber = grpc.pool_channels.operation_sender.subscribe();

    // Spawn a new task for sending new operations
    tokio::spawn(async move {
        if let Some(Ok(request)) = in_stream.next().await {
            let mut request_id = request.id;
            let mut filter = request.query.and_then(|q| q.filter);

            // Spawn a new task for sending new blocks
            loop {
                select! {
                    // Receive a new operation from the subscriber
                     event = subscriber.recv() => {
                        match event {
                            Ok(operation) => {
                                match operation.clone().content.op {
                                    OperationType::Transaction{recipient_address,amount} => {
                                        if is_filtered(&filter, grpc::OperationTypeEnum::Transaction) {
                                            continue
                                        }
                                        grpc::OperationType{
                                            transaction: Some(grpc::Transaction {amount: amount.to_raw(), recipient_address: recipient_address.to_string()}),
                                            ..Default::default()
                                        }
                                    },
                                    OperationType::RollBuy { roll_count } => {
                                        if is_filtered(&filter, grpc::OperationTypeEnum::RollBuy) {
                                            continue
                                        }
                                        grpc::OperationType {
                                            roll_buy: Some( grpc::RollBuy {roll_count}),
                                            ..Default::default()
                                        }
                                    },
                                    OperationType::RollSell { roll_count } => {
                                        if is_filtered(&filter, grpc::OperationTypeEnum::RollSell) {
                                            continue
                                        }
                                        grpc::OperationType {
                                            roll_sell: Some( grpc::RollSell {roll_count}),
                                            ..Default::default()
                                        }
                                    },
                                    OperationType::ExecuteSC { data, max_gas, datastore } => {
                                        if is_filtered(&filter, grpc::OperationTypeEnum::ExecuteSc) {
                                            continue
                                        }

                                       let vec_bytes_map =  datastore.into_iter().map(|(k, v)| {
                                        grpc::BytesMapFieldEntry {
                                                key: k,
                                                value: v
                                            }
                                        }).collect();

                                        grpc::OperationType {
                                            execut_sc: Some( grpc::ExecuteSc {
                                                data,
                                                max_gas,
                                                datastore: vec_bytes_map}),
                                            ..Default::default()
                                        }
                                    },
                                    OperationType::CallSC { target_addr, target_func,  max_gas, param, coins} => {
                                        if is_filtered(&filter, grpc::OperationTypeEnum::CallSc) {
                                            continue
                                        }
                                        grpc::OperationType {
                                            call_sc: Some( grpc::CallSc {
                                                target_addr: target_addr.to_string(),
                                                target_func: target_func.to_string(),
                                                param,
                                                max_gas,
                                                coins: coins.to_raw()
                                            }),
                                            ..Default::default()
                                        }
                                    },
                                };

                                let ret = grpc::SecureShareOperation {
                                    content: Some(operation.content.into()),
                                    //HACK do not map serialized_data
                                    serialized_data: Vec::new(),
                                    signature: operation.signature.to_string(),
                                    content_creator_pub_key: operation.content_creator_pub_key.to_string(),
                                    content_creator_address: operation.content_creator_address.to_string(),
                                    id: operation.id.to_string()
                                };
                                // Send the new operation through the channel
                                if let Err(e) = tx.send(Ok(grpc::NewOperationsStreamResponse {
                                    id: request_id.clone(),
                                    operation: Some(ret)
                                })).await {
                                    error!("failed to send operation : {}", e);
                                    break;
                                }
                            },
                            Err(e) => error!("{}", e)
                        }
                    },
                    // Receive a new message from the in_stream
                    res = in_stream.next() => {
                        match res {
                            Some(res) => {
                                match res {
                                    Ok(data) => {
                                        // Update current filter && request id
                                        filter = data.query
                                        .and_then(|q| q.filter);
                                        request_id = data.id;
                                    },
                                    Err(e) => {
                                        error!("{}", e);
                                        break;
                                    }
                                }
                            },
                            None => {
                                // Client disconnected
                                break;
                            },
                        }
                    }
                }
            }
        } else {
            error!("empty request");
        }
    });

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as NewOperationsStream)
}

/// Return if the type of operation is filtered
///
/// if return true the operation is not sent
fn is_filtered(
    filter_opt: &Option<grpc::NewOperationsStreamFilter>,
    ope_type: grpc::OperationTypeEnum,
) -> bool {
    if let Some(filter) = filter_opt {
        let filtered_ope_ids = &filter.types;
        if filtered_ope_ids.is_empty() {
            return false;
        }
        let id: i32 = ope_type as i32;
        !filtered_ope_ids.contains(&id)
    } else {
        false
    }
}
