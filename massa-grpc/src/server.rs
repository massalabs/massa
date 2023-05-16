// Copyright (c) 2023 MASSA LABS <info@massa.net>

use crate::config::GrpcConfig;
use crate::error::GrpcError;
use futures_util::FutureExt;
use hyper::Method;
use massa_consensus_exports::{ConsensusChannels, ConsensusController};
use massa_execution_exports::{ExecutionChannels, ExecutionController};
use massa_pool_exports::{PoolChannels, PoolController};
use massa_pos_exports::SelectorController;
use massa_proto::massa::api::v1::massa_service_server::MassaServiceServer;
use massa_proto::massa::api::v1::FILE_DESCRIPTOR_SET;
use massa_protocol_exports::ProtocolController;
use massa_storage::Storage;
use tokio::sync::oneshot;
use tonic::{
    codec::CompressionEncoding,
    transport::{Certificate, Identity, ServerTlsConfig},
};
use tonic_health::server::HealthReporter;
use tonic_web::GrpcWebLayer;
use tower_http::cors::{Any, CorsLayer};
use tracing::log::{info, warn};

/// gRPC API content
pub struct MassaGrpc {
    /// link to the consensus component
    pub consensus_controller: Box<dyn ConsensusController>,
    /// link(channels) to the consensus component
    pub consensus_channels: ConsensusChannels,
    /// link to the execution component
    pub execution_controller: Box<dyn ExecutionController>,
    /// link(channels) to the execution component
    pub execution_channels: ExecutionChannels,
    /// link(channels) to the pool component
    pub pool_channels: PoolChannels,
    /// link to the pool component
    pub pool_command_sender: Box<dyn PoolController>,
    /// link to the protocol component
    pub protocol_command_sender: Box<dyn ProtocolController>,
    /// link to the selector component
    pub selector_controller: Box<dyn SelectorController>,
    /// link to the storage component
    pub storage: Storage,
    /// gRPC configuration
    pub grpc_config: GrpcConfig,
    /// node version
    pub version: massa_models::version::Version,
}

impl MassaGrpc {
    /// Start the gRPC API
    pub async fn serve(self, config: &GrpcConfig) -> Result<StopHandle, GrpcError> {
        let mut svc = MassaServiceServer::new(self)
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

        let mut server_builder = tonic::transport::Server::builder()
            .concurrency_limit_per_connection(config.concurrency_limit_per_connection)
            .timeout(config.timeout)
            .initial_stream_window_size(config.initial_stream_window_size)
            .initial_connection_window_size(config.initial_connection_window_size)
            .max_concurrent_streams(config.max_concurrent_streams)
            .tcp_keepalive(config.tcp_keepalive)
            .tcp_nodelay(config.tcp_nodelay)
            .http2_keepalive_interval(config.http2_keepalive_interval)
            .http2_keepalive_timeout(config.http2_keepalive_timeout)
            .http2_adaptive_window(config.http2_adaptive_window)
            .max_frame_size(config.max_frame_size);

        if config.enable_mtls {
            let cert = std::fs::read_to_string(config.server_certificate_path.clone())
                .expect("error, failed to read server certificat");
            let key = std::fs::read_to_string(config.server_private_key_path.clone())
                .expect("error, failed to read server private key");
            let server_identity = Identity::from_pem(cert, key);

            let client_ca_cert =
                std::fs::read_to_string(config.client_certificate_authority_root_path.clone())
                    .expect("error, failed to read client certificate authority root");
            let client_ca_cert = Certificate::from_pem(client_ca_cert);

            let tls = ServerTlsConfig::new()
                .identity(server_identity)
                .client_ca_root(client_ca_cert);

            server_builder = server_builder
                .tls_config(tls)
                .expect("error, failed to setup mTLS");

            info!("gRPC mTLS enabled");
        }

        let reflection_service_opt = if config.enable_reflection {
            let reflection_service = tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
                .build()?;

            Some(reflection_service)
        } else {
            None
        };

        let health_service_opt = if config.enable_health {
            let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
            health_reporter
                .set_serving::<MassaServiceServer<MassaGrpc>>()
                .await;
            tokio::spawn(massa_service_status(health_reporter.clone()));
            info!("gRPC health service enabled");
            Some(health_service)
        } else {
            None
        };

        if config.accept_http1 {
            if config.enable_cors {
                let cors = CorsLayer::new()
                    // Allow `GET`, `POST` and `OPTIONS` when accessing the resource
                    .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
                    // Allow requests from any origin
                    .allow_origin(Any)
                    .allow_headers([hyper::header::CONTENT_TYPE]);

                let router_with_http1 = server_builder
                    .accept_http1(true)
                    .layer(cors)
                    .layer(GrpcWebLayer::new())
                    .add_optional_service(reflection_service_opt)
                    .add_optional_service(health_service_opt)
                    .add_service(svc);

                tokio::spawn(
                    router_with_http1.serve_with_shutdown(config.bind, shutdown_recv.map(drop)),
                );
            } else {
                let router_with_http1 = server_builder
                    .accept_http1(true)
                    .layer(GrpcWebLayer::new())
                    .add_optional_service(reflection_service_opt)
                    .add_optional_service(health_service_opt)
                    .add_service(svc);

                tokio::spawn(
                    router_with_http1.serve_with_shutdown(config.bind, shutdown_recv.map(drop)),
                );
            }
        } else {
            let router = server_builder
                .add_optional_service(reflection_service_opt)
                .add_optional_service(health_service_opt)
                .add_service(svc);

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
            info!("gRPC API stop signal sent successfully");
        }
    }
}

/// Massa service health check implementation
async fn massa_service_status(mut reporter: HealthReporter) {
    //TODO add a complete health check based on Massa modules health
    reporter
        .set_serving::<MassaServiceServer<MassaGrpc>>()
        .await;
}
