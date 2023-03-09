use massa_proto::massa::api::v1::{self as grpc};
use std::pin::Pin;
use tonic::codegen::futures_core;

/// type declaration for SendBlockStream
pub type SendBlocksStream = Pin<
    Box<
        dyn futures_core::Stream<Item = Result<grpc::SendBlocksResponse, tonic::Status>>
            + Send
            + 'static,
    >,
>;
