use crate::error::{match_for_io_error, GrpcError};
use crate::service::MassaGrpcService;
use futures_util::StreamExt;
use massa_models::block::FilledBlock;
use massa_proto::massa::api::v1::{NewFilledBlocksStreamRequest, NewFilledBlocksStreamResponse};
use std::io::ErrorKind;
use std::pin::Pin;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::{error, warn};

/// Type declaration for NewFilledBlocksStream
pub type NewFilledBlocksStream = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<NewFilledBlocksStreamResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// New filled blocks
pub(crate) async fn new_filled_blocks(
    grpc: &MassaGrpcService,
    request: Request<Streaming<NewFilledBlocksStreamRequest>>,
) -> Result<NewFilledBlocksStream, GrpcError> {
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    let mut in_stream = request.into_inner();
    let mut subscriber = grpc.consensus_channels.filled_block_sender.subscribe();

    tokio::spawn(async move {
        let mut request_id = String::new();
        loop {
            select! {
                 event = subscriber.recv() => {
                    match event {
                        Ok(share_block) => {
                            let massa_block = share_block as FilledBlock;
                            if let Err(e) = tx.send(Ok(NewFilledBlocksStreamResponse {
                                    id: request_id.clone(),
                                    filled_block: Some(massa_block.into())
                            })).await {
                                error!("failed to send new block : {}", e);
                                break;
                            }
                        },
                        Err(e) => {error!("error on receive new block : {}", e)}
                    }
                },
                res = in_stream.next() => {
                    match res {
                        Some(res) => {
                            match res {
                                Ok(data) => {
                                    request_id = data.id
                                },
                                Err(err) => {
                                    if let Some(io_err) = match_for_io_error(&err) {
                                        if io_err.kind() == ErrorKind::BrokenPipe {
                                            warn!("client disconnected, broken pipe: {}", io_err);
                                            break;
                                        }
                                    }
                                    error!("{}", err);
                                    if let Err(e) = tx.send(Err(err)).await {
                                        error!("failed to send back new_blocks error response: {}", e);
                                        break;
                                    }
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
    });

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Box::pin(out_stream) as NewFilledBlocksStream)
}
