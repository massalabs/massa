// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaGrpc;
use futures_util::StreamExt;
use massa_proto::massa::api::v1 as grpc;
use std::pin::Pin;
use tokio::select;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};
use tracing::log::error;

/// Type declaration for NewScExecutionOutputs
pub type NewScExecutionOutputsStreamType = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc::NewScExecutionOutputsResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Creates a new stream of new produced and received smart contracts execution outputs
pub(crate) async fn new_sc_execution_outputs(
    grpc: &MassaGrpc,
    request: Request<Streaming<grpc::NewScExecutionOutputsRequest>>,
) -> Result<NewScExecutionOutputsStreamType, GrpcError> {
    unimplemented!("new_sc_execution_outputs")
}
