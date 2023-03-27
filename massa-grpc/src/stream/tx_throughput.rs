use crate::{error::GrpcError, service::MassaGrpcService};
use futures_util::StreamExt;
use massa_proto::massa::api::v1::{self as grpc, GetTransactionsThroughputResponse};
use std::pin::Pin;
use std::time::Duration;
use tokio::{select, time};
use tonic::codegen::futures_core;
use tracing::log::error;

const INTERVAL_TIME: u64 = 10; // second

/// type declaration for StreamTransactionsThroughputStream
pub type TransactionsThroughputStream = Pin<
    Box<
        dyn futures_core::Stream<
                Item = Result<grpc::GetTransactionsThroughputResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;

pub(crate) async fn transactions_throughput(
    grpc: &MassaGrpcService,
    request: tonic::Request<tonic::Streaming<grpc::TransactionsThroughputStreamRequest>>,
) -> Result<TransactionsThroughputStream, GrpcError> {
    let (tx, rx) = tokio::sync::mpsc::channel(grpc.grpc_config.max_channel_size);
    let mut in_stream = request.into_inner();
    let ctrl = grpc.execution_controller.clone();

    tokio::spawn(async move {
        let mut request_id = "".to_string();
        let mut interval = time::interval(Duration::from_secs(INTERVAL_TIME));

        loop {
            select! {
                res = in_stream.next() => {
                    match res {
                        Some(Ok(req)) => {
                            request_id = req.id;
                            // update interval tick
                            let new_timer = req.interval.unwrap_or(INTERVAL_TIME);
                            interval = time::interval(Duration::from_secs(new_timer));
                            interval.reset();
                        },
                        _ => {
                            // client disconnected
                            break;
                        }
                    }
                },
                _ = interval.tick() => {
                    let stats = ctrl.get_stats();
                    let nb_sec_range = stats
                        .time_window_end
                        .saturating_sub(stats.time_window_start)
                        .to_duration()
                        .as_secs();

                    let tx_s = stats
                        .final_executed_operations_count
                        .checked_div(nb_sec_range as usize)
                        .unwrap_or_default() as u32;

                    if let Err(e) = tx
                        .send(Ok(GetTransactionsThroughputResponse {
                            id: request_id.clone(),
                            tx_s,
                        }))
                        .await
                    {
                        error!("failed to send back tx_s response: {}", e);
                        break;
                    }
                }
            }
        }
    });

    let out_stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    Ok(Box::pin(out_stream) as TransactionsThroughputStream)
}
