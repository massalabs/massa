use crate::error::GrpcError;
use crate::handler::MassaGrpcService;
use futures_util::StreamExt;
use massa_models::operation::{OperationType, SecureShareOperation};
use massa_proto::massa::api::v1::{
    self as grpc, OperationStreamFilterType, SubscribeNewOperationsStreamRequest,
    SubscribeNewOperationsStreamResponse,
};
use std::pin::Pin;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::error;

/// type declaration for StreamTransactionsThroughputStream
pub type SubscribeNewOperationsStream = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<SubscribeNewOperationsStreamResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

pub(crate) async fn subscribe_new_operations(
    grpc: &MassaGrpcService,
    request: Request<Streaming<SubscribeNewOperationsStreamRequest>>,
) -> Result<SubscribeNewOperationsStream, GrpcError> {
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    let mut in_stream = request.into_inner();
    let mut subscriber = grpc.pool_channels.operation_sender.subscribe();

    tokio::spawn(async move {
        if let Some(Ok(request)) = in_stream.next().await {
            let mut request_id = request.id;
            let mut filter = request.filter;

            loop {
                select! {
                     event = subscriber.recv() => {
                        match event {
                            Ok(ope) => {
                                let operation = ope as SecureShareOperation;

                                let ope_type = match operation.content.op {
                                    OperationType::Transaction{recipient_address,amount} => {
                                        if is_filtered(&filter, OperationStreamFilterType::Transaction) {
                                            continue
                                        }
                                        grpc::OperationType{
                                            transaction: Some(grpc::Transaction {amount: amount.to_string(), recipient_address: recipient_address.to_string()}),
                                            ..Default::default()
                                        }
                                    },
                                    OperationType::RollBuy { roll_count } => {
                                        if is_filtered(&filter, OperationStreamFilterType::RollBuy) {
                                            continue
                                        }
                                        grpc::OperationType {
                                            roll_buy: Some( grpc::RollBuy {roll_count}),
                                            ..Default::default()
                                        }
                                    },
                                    OperationType::RollSell { roll_count } => {
                                        if is_filtered(&filter, OperationStreamFilterType::RollSell) {
                                            continue
                                        }
                                        grpc::OperationType {
                                            roll_sell: Some( grpc::RollSell {roll_count}),
                                            ..Default::default()
                                        }
                                    },
                                    OperationType::ExecuteSC { .. } => {
                                        if is_filtered(&filter, OperationStreamFilterType::ExecuteSc) {
                                            continue
                                        }

                                        // missing field : gas_price , coins

                                        todo!()
                                        // grpc::OperationType {
                                        //     execut_sc: Some( grpc::ExecuteSc {data, max_gas,}),
                                        //     ..Default::default()
                                        // }
                                    },
                                    OperationType::CallSC { target_addr, target_func,  max_gas, param, coins} => {
                                        if is_filtered(&filter, OperationStreamFilterType::CallSc) {
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
                                    content: Some(ope_type),
                                    signature: operation.signature.to_string(),
                                    content_creator_pub_key: operation.content_creator_pub_key.to_string(),
                                    content_creator_address: operation.content_creator_address.to_string(),
                                    id: operation.id.to_string()
                                };
                                if let Err(e) = tx.send(Ok(SubscribeNewOperationsStreamResponse {
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
                    res = in_stream.next() => {
                            match res {
                                Some(res) => {
                                    match res {
                                        Ok(data) => {
                                            dbg!(&data);
                                            // update current filter && request id
                                            filter = data.filter;
                                            request_id = data.id;
                                        },
                                        Err(e) => {
                                            error!("{}", e);
                                            break;
                                        }
                                    }
                                },
                                None => {
                                    // client disconnected
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
    Ok(Box::pin(out_stream) as SubscribeNewOperationsStream)
}

/// Return if the type of operation is filtered
///
/// if return true the operation is not sent
fn is_filtered(filter_ids: &Vec<i32>, ope_type: OperationStreamFilterType) -> bool {
    if filter_ids.is_empty() {
        return false;
    }

    let id: i32 = ope_type as i32;
    !filter_ids.contains(&id)
}
