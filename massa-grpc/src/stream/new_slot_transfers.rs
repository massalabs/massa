use futures_util::StreamExt;
use massa_models::slot::Slot;
use massa_proto_rs::massa::api::v1::{self as grpc_api, FinalityLevel};
use std::pin::Pin;
use tokio::select;
use tonic::{Request, Streaming};
use tracing::{error, warn};

use crate::error::match_for_io_error;
use crate::{error::GrpcError, server::MassaPublicGrpc};

/// Type declaration for NewSlotTransfers
pub type NewSlotTransfersStreamType = Pin<
    Box<
        dyn futures_util::Stream<Item = Result<grpc_api::NewSlotTransfersResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Creates a new stream of new slots transfers
pub(crate) async fn new_slot_transfers(
    grpc: &MassaPublicGrpc,
    request: Request<Streaming<grpc_api::NewSlotTransfersRequest>>,
) -> Result<NewSlotTransfersStreamType, GrpcError> {
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    // Extract the incoming stream of abi call stacks messages
    let mut in_stream = request.into_inner();

    // Subscribe to the new slot execution events channel
    #[cfg(feature = "execution-trace")]
    let mut subscriber = grpc
        .execution_channels
        .slot_execution_traces_sender
        .subscribe();
    #[cfg(not(feature = "execution-trace"))]
    let (mut subscriber, _receiver) = {
        let (subscriber_, receiver) = tokio::sync::broadcast::channel::<SlotAbiCallStack>(0);
        (subscriber_.subscribe(), receiver)
    };

    tokio::spawn(async move {
        let mut finality = FinalityLevel::Unspecified;
        loop {
            select! {
                // Receive a new slot execution traces from the subscriber
                event = subscriber.recv() => {
                    match event {
                        Ok((_massa_slot_execution_trace, received_finality)) => {
                            if (finality == FinalityLevel::Final && received_finality != true) ||
                                (finality == FinalityLevel::Candidate && received_finality != false) {
                                continue;
                            }

                            let ret = grpc_api::NewSlotTransfersResponse {
                                slot: Some(Slot::new(0, 0).into()),
                                transfers: vec![]
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
    });
    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as NewSlotTransfersStreamType)
}
