use std::pin::Pin;

#[cfg(feature = "execution-trace")]
use crate::{error::GrpcError, server::MassaPublicGrpc};
#[cfg(feature = "execution-trace")]
use tonic::{Request, Streaming};

/// Type declaration for NewSlotTransfers
pub type NewSlotTransfersStreamType = Pin<
    Box<
        dyn futures_util::Stream<
                Item = Result<
                    massa_proto_rs::massa::api::v1::NewSlotTransfersResponse,
                    tonic::Status,
                >,
            > + Send
            + 'static,
    >,
>;

#[cfg(feature = "execution-trace")]
/// Creates a new stream of new slots transfers
pub(crate) async fn new_slot_transfers(
    grpc: &MassaPublicGrpc,
    request: Request<Streaming<massa_proto_rs::massa::api::v1::NewSlotTransfersRequest>>,
) -> Result<NewSlotTransfersStreamType, GrpcError> {
    use crate::error::match_for_io_error;
    use futures_util::StreamExt;
    use massa_proto_rs::massa::api::v1::{self as grpc_api, FinalityLevel, TransferInfo};
    use tokio::select;
    use tracing::{error, warn};

    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Extract the incoming stream of abi call stacks messages
    let mut in_stream = request.into_inner();

    // Subscribe to the new slot execution events channel
    let mut subscriber = grpc
        .execution_channels
        .slot_execution_traces_sender
        .subscribe();

    tokio::spawn({
        let execution_controller = grpc.execution_controller.clone();
        async move {
            let mut finality = FinalityLevel::Unspecified;
            loop {
                select! {
                    // Receive a new slot execution traces from the subscriber
                    event = subscriber.recv() => {
                        match event {
                            Ok((massa_slot_execution_trace, is_final)) => {
                                if (finality == FinalityLevel::Final && !is_final) ||
                                    (finality == FinalityLevel::Candidate && is_final) ||
                                    (finality == FinalityLevel::Unspecified && !is_final) {
                                    continue;
                                }
                                let mut ret_transfers = Vec::new();
                                // flatten & filter transfer trace in asc_call_stacks

                                let abi_transfer_1 = "assembly_script_transfer_coins".to_string();
                                let abi_transfer_2 = "assembly_script_transfer_coins_for".to_string();
                                let abi_transfer_3 = "abi_transfer_coins".to_string();
                                let transfer_abi_names = vec![abi_transfer_1, abi_transfer_2, abi_transfer_3];
                                for (i, asc_call_stack) in massa_slot_execution_trace.asc_call_stacks.iter().enumerate() {
                                    for abi_trace in asc_call_stack {
                                        let only_transfer = abi_trace.flatten_filter(&transfer_abi_names);
                                        for transfer in only_transfer {
                                            let (t_from, t_to, t_amount) = transfer.parse_transfer();
                                            ret_transfers.push(TransferInfo {
                                                from: t_from.clone(),
                                                to: t_to.clone(),
                                                amount: t_amount,
                                                operation_id_or_asc_index: Some(
                                                    grpc_api::transfer_info::OperationIdOrAscIndex::AscIndex(i as u64),
                                                ),
                                            });
                                        }
                                    }
                                }

                                for deferred_call_call_stack in massa_slot_execution_trace.deferred_call_stacks {
                                    let deferred_call_id = deferred_call_call_stack.0;
                                    let deferred_call_call_stack = deferred_call_call_stack.1;
                                    for abi_trace in deferred_call_call_stack {
                                        let only_transfer = abi_trace.flatten_filter(&transfer_abi_names);
                                        for transfer in only_transfer {
                                            let (t_from, t_to, t_amount) = transfer.parse_transfer();
                                            ret_transfers.push(TransferInfo {
                                                from: t_from.clone(),
                                                to: t_to.clone(),
                                                amount: t_amount,
                                                operation_id_or_asc_index: Some(
                                                    grpc_api::transfer_info::OperationIdOrAscIndex::DeferredCallId(deferred_call_id.to_string()),
                                                ),
                                            });
                                        }
                                    }
                                }

                                for op_call_stack in massa_slot_execution_trace.operation_call_stacks {
                                    let op_id = op_call_stack.0;
                                    let op_call_stack = op_call_stack.1;
                                    for abi_trace in op_call_stack {
                                        let only_transfer = abi_trace.flatten_filter(&transfer_abi_names);
                                        for transfer in only_transfer {
                                            let (t_from, t_to, t_amount) = transfer.parse_transfer();
                                            ret_transfers.push(TransferInfo {
                                                from: t_from.clone(),
                                                to: t_to.clone(),
                                                amount: t_amount,
                                                operation_id_or_asc_index: Some(
                                                    grpc_api::transfer_info::OperationIdOrAscIndex::OperationId(op_id.to_string()),
                                                ),
                                            });
                                        }
                                    }
                                }
                                let transfers =
                                    execution_controller
                                    .get_transfers_for_slot(massa_slot_execution_trace.slot);
                                if let Some(transfers) = transfers {
                                    for transfer in transfers {
                                        ret_transfers.push(TransferInfo {
                                            from: transfer.from.to_string(),
                                            to: transfer.to.to_string(),
                                            amount: transfer.amount.to_raw(),
                                            operation_id_or_asc_index: Some(
                                                grpc_api::transfer_info::OperationIdOrAscIndex::OperationId(
                                                    transfer.op_id.to_string(),
                                                ),
                                            ),
                                        });
                                    }
                                }
                                let ret = grpc_api::NewSlotTransfersResponse {
                                    slot: Some(massa_slot_execution_trace.slot.into()),
                                    transfers: ret_transfers,
                                };

                                if let Err(e) = tx.send(Ok(ret)).await {
                                    error!("failed to send new slot execution trace: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("error on receive new slot execution trace : {}", e)
                            }
                        }
                    }
                    // Receive a new message from the in_stream
                    res = in_stream.next() => {
                        match res {
                            Some(res) => {
                                match res {
                                    Ok(message) => {
                                        finality = message.finality_level();
                                    },
                                    Err(e) => {
                                        // Any io error -> break
                                        if let Some(io_err) = match_for_io_error(&e) {
                                            warn!("client disconnected, broken pipe: {}", io_err);
                                            break;
                                        }
                                        error!("{}", e);
                                        if let Err(e2) = tx.send(Err(e)).await {
                                            error!("failed to send back error response: {}", e2);
                                            break;
                                        }
                                    }
                                }
                            }
                            None => {
                                // the client has disconnected
                                break;
                            }
                        }
                    }
                }
            }
        }
    });
    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as NewSlotTransfersStreamType)
}
