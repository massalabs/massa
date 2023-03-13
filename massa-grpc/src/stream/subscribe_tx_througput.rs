use crate::api::MassaGrpcService;
use massa_proto::massa::api::v1::{self as grpc};
use std::pin::Pin;
use tonic::codegen::futures_core;

/// type declaration for StreamTransactionsThroughputStream
pub type SendTransactionsThroughputStream = Pin<
    Box<
        dyn futures_core::Stream<
                Item = Result<grpc::GetTransactionsThroughputResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;

pub(crate) async fn send_transactions_throughput(
    grpc: &MassaGrpcService,
    request: tonic::Request<tonic::Streaming<grpc::GetTransactionsThroughputRequest>>,
) -> Result<SendOperationsStream, GrpcError> {
}
