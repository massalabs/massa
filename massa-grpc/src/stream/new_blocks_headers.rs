use crate::error::{match_for_io_error, GrpcError};
use crate::service::MassaGrpcService;
use futures_util::StreamExt;
use massa_models::block_header::BlockHeader;
use massa_models::block_id::BlockId;
use massa_models::secure_share::SecureShare;
use massa_proto::massa::api::v1::{NewBlocksHeadersStreamRequest, NewBlocksHeadersStreamResponse};
use std::io::ErrorKind;
use std::pin::Pin;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::{error, warn};

/// Type declaration for NewBlocksHeadersStream
pub type NewBlocksHeadersStream = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<NewBlocksHeadersStreamResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// New blocks headers
pub(crate) async fn new_blocks_headers(
    grpc: &MassaGrpcService,
    request: Request<Streaming<NewBlocksHeadersStreamRequest>>,
) -> Result<NewBlocksHeadersStream, GrpcError> {
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    let mut in_stream = request.into_inner();
    let mut subscriber = grpc.consensus_channels.block_header_sender.subscribe();

    tokio::spawn(async move {
        let mut request_id = String::new();
        loop {
            select! {
                 event = subscriber.recv() => {
                    match event {
                        Ok(share_block_header) => {
                            let massa_block_header = share_block_header as SecureShare<BlockHeader, BlockId>;
                            if let Err(e) = tx.send(Ok(NewBlocksHeadersStreamResponse {
                                    id: request_id.clone(),
                                    block_header: Some(massa_block_header.into())
                            })).await {
                                error!("failed to send new block header : {}", e);
                                break;
                            }
                        },
                        Err(e) => {error!("error on receive new block header : {}", e)}
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
                                        error!("failed to send back new_blocks_headers error response: {}", e);
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
    Ok(Box::pin(out_stream) as NewBlocksHeadersStream)
}
