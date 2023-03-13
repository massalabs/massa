//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//! gRPC API for a massa-node

use crate::{config::GrpcConfig, error::GrpcError};
use massa_consensus_exports::{ConsensusChannels, ConsensusController};
use massa_execution_exports::ExecutionController;
use massa_pool_exports::{PoolChannels, PoolController};
use massa_pos_exports::SelectorController;
use massa_proto::massa::api::v1::{
    self as grpc, grpc_server::GrpcServer, GetNextBlockBestParentsRequest,
    GetNextBlockBestParentsResponse, FILE_DESCRIPTOR_SET,
};

use crate::business::{
    get_datastore_entries, get_next_block_best_parents, get_selector_draws, get_version,
};
use crate::stream::{
    send_blocks::{send_blocks, SendBlocksStream},
    send_endorsements::{send_endorsements, SendEndorsementsStream},
    send_operations::{send_operations, SendOperationsStream},
};
use futures_util::FutureExt;
use massa_protocol_exports::ProtocolCommandSender;
use massa_storage::Storage;
use tokio::sync::oneshot;
use tonic::codegen::CompressionEncoding;
use tonic::{Request, Response, Status};
use tonic_web::GrpcWebLayer;
use tracing::log::{info, warn};

/// gRPC API content
pub struct MassaGrpcService {
    /// link(channels) to the consensus component
    pub consensus_controller: Box<dyn ConsensusController>,
    /// link(channels) to the consensus component
    pub consensus_channels: ConsensusChannels,
    /// link to the execution component
    pub execution_controller: Box<dyn ExecutionController>,
    /// link(channels) to the pool component
    pub pool_channels: PoolChannels,
    /// link to the pool component
    pub pool_command_sender: Box<dyn PoolController>,
    /// link(channels) to the protocol component
    pub protocol_command_sender: ProtocolCommandSender,
    /// link to the selector component
    pub selector_controller: Box<dyn SelectorController>,
    /// link to the storage component
    pub storage: Storage,
    /// gRPC configuration
    pub grpc_config: GrpcConfig,
    /// node version
    pub version: massa_models::version::Version,
}

impl MassaGrpcService {
    /// Start the gRPC API
    pub async fn serve(self, config: &GrpcConfig) -> Result<StopHandle, GrpcError> {
        let mut svc = GrpcServer::new(self)
            .max_decoding_message_size(config.max_decoding_message_size)
            .max_encoding_message_size(config.max_encoding_message_size);

        if let Some(encoding) = &config.accept_compressed {
            if encoding.eq_ignore_ascii_case("Gzip") {
                svc = svc.accept_compressed(CompressionEncoding::Gzip);
            };
        }

        if let Some(encoding) = &config.send_compressed {
            if encoding.eq_ignore_ascii_case("Gzip") {
                svc = svc.send_compressed(CompressionEncoding::Gzip);
            };
        }

        let (shutdown_send, shutdown_recv) = oneshot::channel::<()>();

        if config.accept_http1 {
            let mut router_with_http1 = tonic::transport::Server::builder()
                .accept_http1(true)
                .layer(GrpcWebLayer::new())
                .add_service(svc);

            if config.enable_reflection {
                let reflection_service = tonic_reflection::server::Builder::configure()
                    .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
                    .build()?;

                router_with_http1 = router_with_http1.add_service(reflection_service);
            }

            //  // todo get config runtime
            //  match self.cfg.tokio_runtime.take() {
            //      Some(rt) => rt.spawn(self.start_inner(methods, stop_handle)),
            //      None => tokio::spawn(self.start_inner(methods, stop_handle)),
            //  };

            tokio::spawn(
                router_with_http1.serve_with_shutdown(config.bind, shutdown_recv.map(drop)),
            );
        } else {
            let mut router = tonic::transport::Server::builder().add_service(svc);

            if config.enable_reflection {
                let reflection_service = tonic_reflection::server::Builder::configure()
                    .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
                    .build()?;

                router = router.add_service(reflection_service);
            }

            //  // todo get config runtime
            //  match self.cfg.tokio_runtime.take() {
            //      Some(rt) => rt.spawn(self.start_inner(methods, stop_handle)),
            //      None => tokio::spawn(self.start_inner(methods, stop_handle)),
            //  };

            tokio::spawn(router.serve_with_shutdown(config.bind, shutdown_recv.map(drop)));
        }

        Ok(StopHandle {
            stop_cmd_sender: shutdown_send,
        })
    }
}

/// Used to be able to stop the gRPC API
pub struct StopHandle {
    stop_cmd_sender: oneshot::Sender<()>,
}

impl StopHandle {
    /// stop the gRPC API gracefully
    pub fn stop(self) {
        if let Err(e) = self.stop_cmd_sender.send(()) {
            warn!("gRPC API thread panicked: {:?}", e);
        } else {
            info!("gRPC API finished cleanly");
        }
    }
}

#[tonic::async_trait]
impl grpc::grpc_server::Grpc for MassaGrpcService {
    /// Handler for get multiple datastore entries.
    async fn get_datastore_entries(
        &self,
        request: tonic::Request<grpc::GetDatastoreEntriesRequest>,
    ) -> Result<tonic::Response<grpc::GetDatastoreEntriesResponse>, tonic::Status> {
        match get_datastore_entries(self, request) {
            Ok(response) => Ok(tonic::Response::new(response)),
            Err(e) => Err(e.into()),
        }
    }

    /// Handler for get version
    async fn get_version(
        &self,
        request: tonic::Request<grpc::GetVersionRequest>,
    ) -> Result<tonic::Response<grpc::GetVersionResponse>, tonic::Status> {
        match get_version(self, request) {
            Ok(response) => Ok(tonic::Response::new(response)),
            Err(e) => Err(e.into()),
        }
    }

    /// Handler for get selector draws
    async fn get_selector_draws(
        &self,
        request: Request<grpc::GetSelectorDrawsRequest>,
    ) -> Result<Response<grpc::GetSelectorDrawsResponse>, tonic::Status> {
        match get_selector_draws(self, request) {
            Ok(response) => Ok(tonic::Response::new(response)),
            Err(e) => Err(e.into()),
        }
    }

    /// Handler for get_next_block_best_parents
    async fn get_next_block_best_parents(
        &self,
        request: Request<GetNextBlockBestParentsRequest>,
    ) -> Result<Response<GetNextBlockBestParentsResponse>, Status> {
        match get_next_block_best_parents(self, request).await {
            Ok(response) => Ok(Response::new(response)),
            Err(e) => Err(e.into()),
        }
    }

    // ███████╗████████╗██████╗ ███████╗ █████╗ ███╗   ███╗
    // ██╔════╝╚══██╔══╝██╔══██╗██╔════╝██╔══██╗████╗ ████║
    // ███████╗   ██║   ██████╔╝█████╗  ███████║██╔████╔██║
    // ╚════██║   ██║   ██╔══██╗██╔══╝  ██╔══██║██║╚██╔╝██║
    // ███████║   ██║   ██║  ██║███████╗██║  ██║██║ ╚═╝ ██║

    type SendBlocksStream = SendBlocksStream;
    type SendEndorsementsStream = SendEndorsementsStream;
    type SendOperationsStream = SendOperationsStream;

    /// Handler for send_blocks_stream
    async fn send_blocks(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::SendBlocksRequest>>,
    ) -> Result<tonic::Response<Self::SendBlocksStream>, tonic::Status> {
        match send_blocks(self, request).await {
            Ok(res) => Ok(tonic::Response::new(res)),
            Err(e) => Err(e.into()),
        }
    }

    /// Handler for send_endorsements
    async fn send_endorsements(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::SendEndorsementsRequest>>,
    ) -> Result<tonic::Response<Self::SendEndorsementsStream>, tonic::Status> {
        match send_endorsements(self, request).await {
            Ok(res) => Ok(tonic::Response::new(res)),
            Err(e) => Err(e.into()),
        }
    }

    /// Handler for send_operations
    async fn send_operations(
        &self,
        request: tonic::Request<tonic::Streaming<grpc::SendOperationsRequest>>,
    ) -> Result<tonic::Response<Self::SendOperationsStream>, tonic::Status> {
        match send_operations(self, request).await {
            Ok(res) => Ok(tonic::Response::new(res)),
            Err(e) => Err(e.into()),
        }
    }
}
