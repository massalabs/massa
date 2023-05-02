// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::error::GrpcError;
use crate::server::MassaGrpc;
use massa_proto::massa::api::v1 as grpc;
use std::pin::Pin;
use tonic::codegen::futures_core;
use tonic::{Request, Streaming};

/// Type declaration for NewBlockCliques
pub type NewBlockCliquesStreamType = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc::NewBlockCliquesResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;

/// Creates a new stream of new produced and received blockcliques
pub(crate) async fn new_block_cliques(
    _grpc: &MassaGrpc,
    _request: Request<Streaming<grpc::NewBlockCliquesRequest>>,
) -> Result<NewBlockCliquesStreamType, GrpcError> {
    unimplemented!("new_blockcliques")
}
