use crate::config::GrpcConfig;
use crate::error::GrpcError;
use futures_util::FutureExt;
use massa_consensus_exports::{ConsensusChannels, ConsensusController};
use massa_execution_exports::ExecutionController;
use massa_pool_exports::{PoolChannels, PoolController};
use massa_pos_exports::SelectorController;
use massa_proto::massa::api::v1::grpc_server::GrpcServer;
use massa_proto::massa::api::v1::FILE_DESCRIPTOR_SET;
use massa_protocol_exports::ProtocolCommandSender;
use massa_storage::Storage;
use tokio::sync::oneshot;
use tonic::codec::CompressionEncoding;
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
