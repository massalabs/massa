use massa_proto_rs::massa::api::v1::{self as grpc_api};
use std::pin::Pin;

/// Type declaration for NewSlotExecutionOutputs
pub type NewSlotABICallStacksStreamType = Pin<
    Box<
        dyn futures_util::Stream<
                Item = Result<grpc_api::NewSlotAbiCallStacksResponse, tonic::Status>,
            > + Send
            + 'static,
    >,
>;
