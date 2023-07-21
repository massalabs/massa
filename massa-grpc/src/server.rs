// Copyright (c) 2023 MASSA LABS <info@massa.net>

use massa_bootstrap::white_black_list::SharedWhiteBlackList;
use massa_models::node::NodeId;
use massa_versioning::keypair_factory::KeyPairFactory;
use massa_versioning::versioning::MipStore;
use parking_lot::RwLock;
use std::convert::Infallible;
use std::sync::{Arc, Condvar, Mutex};

use crate::config::GrpcConfig;
use crate::error::GrpcError;
use futures_util::FutureExt;
use hyper::service::Service;
use hyper::{Body, Method, Request, Response};
use massa_consensus_exports::{ConsensusChannels, ConsensusController};
use massa_execution_exports::{ExecutionChannels, ExecutionController};
use massa_pool_exports::{PoolChannels, PoolController};
use massa_pos_exports::SelectorController;
use massa_proto_rs::massa::api::v1::FILE_DESCRIPTOR_SET;
use massa_proto_rs::massa::api::v1::{
    private_service_server::PrivateServiceServer, public_service_server::PublicServiceServer,
};
use massa_protocol_exports::{ProtocolConfig, ProtocolController};
use massa_storage::Storage;

use massa_wallet::Wallet;
use openssl::asn1::Asn1Time;
use openssl::bn::{BigNum, MsbOption};
use openssl::error::ErrorStack;
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, PKeyRef, Private};
use openssl::rsa::Rsa;
use openssl::x509::extension::{
    AuthorityKeyIdentifier, BasicConstraints, KeyUsage, SubjectAlternativeName,
    SubjectKeyIdentifier,
};
use openssl::x509::{X509NameBuilder, X509Ref, X509Req, X509ReqBuilder, X509};

use tokio::sync::oneshot;
use tonic::body::BoxBody;
use tonic::codegen::CompressionEncoding;
use tonic::transport::NamedService;
use tonic::transport::{Certificate, Identity, ServerTlsConfig};
use tonic_health::server::HealthReporter;
use tonic_web::GrpcWebLayer;
use tower_http::cors::{Any, CorsLayer};
use tracing::log::{info, warn};

/// gRPC PRIVATE API content
pub struct MassaPrivateGrpc {
    /// link to the consensus component
    pub consensus_controller: Box<dyn ConsensusController>,
    /// link to the execution component
    pub execution_controller: Box<dyn ExecutionController>,
    /// link to the pool component
    pub pool_controller: Box<dyn PoolController>,
    /// link to the protocol component
    pub protocol_controller: Box<dyn ProtocolController>,
    /// Mechanism by which to gracefully shut down.
    /// To be a clone of the same pair provided to the ctrlc handler.
    pub stop_cv: Arc<(Mutex<bool>, Condvar)>,
    /// User wallet
    pub node_wallet: Arc<RwLock<Wallet>>,
    /// gRPC configuration
    pub grpc_config: GrpcConfig,
    /// Massa protocol configuration
    pub protocol_config: ProtocolConfig,
    /// our node id
    pub node_id: NodeId,
    /// database for all MIP info
    pub mip_store: MipStore,
    /// node version
    pub version: massa_models::version::Version,
    /// white/black list of bootstrap
    pub bs_white_black_list: Option<SharedWhiteBlackList<'static>>,
}

impl MassaPrivateGrpc {
    /// Start the gRPC PRIVATE API
    pub async fn serve(self, config: &GrpcConfig) -> Result<StopHandle, GrpcError> {
        let mut service = PrivateServiceServer::new(self)
            .max_decoding_message_size(config.max_decoding_message_size)
            .max_encoding_message_size(config.max_encoding_message_size);

        if let Some(encoding) = &config.accept_compressed {
            if encoding.eq_ignore_ascii_case("Gzip") {
                service = service.accept_compressed(CompressionEncoding::Gzip);
            };
        }

        if let Some(encoding) = &config.send_compressed {
            if encoding.eq_ignore_ascii_case("Gzip") {
                service = service.send_compressed(CompressionEncoding::Gzip);
            };
        }

        serve(service, config).await
    }
}

/// gRPC PUBLIC API content
pub struct MassaPublicGrpc {
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
    pub pool_controller: Box<dyn PoolController>,
    /// link to the protocol component
    pub protocol_controller: Box<dyn ProtocolController>,
    /// link to the selector component
    pub selector_controller: Box<dyn SelectorController>,
    /// link to the storage component
    pub storage: Storage,
    /// gRPC configuration
    pub grpc_config: GrpcConfig,
    /// Massa protocol configuration
    pub protocol_config: ProtocolConfig,
    /// our node id
    pub node_id: NodeId,
    /// node version
    pub version: massa_models::version::Version,
    /// keypair factory
    pub keypair_factory: KeyPairFactory,
}

impl MassaPublicGrpc {
    /// Start the gRPC PUBLIC API
    pub async fn serve(self, config: &GrpcConfig) -> Result<StopHandle, GrpcError> {
        let mut service = PublicServiceServer::new(self)
            .max_decoding_message_size(config.max_decoding_message_size)
            .max_encoding_message_size(config.max_encoding_message_size);

        if let Some(encoding) = &config.accept_compressed {
            if encoding.eq_ignore_ascii_case("Gzip") {
                service = service.accept_compressed(CompressionEncoding::Gzip);
            };
        }

        if let Some(encoding) = &config.send_compressed {
            if encoding.eq_ignore_ascii_case("Gzip") {
                service = service.send_compressed(CompressionEncoding::Gzip);
            };
        }

        serve(service, config).await
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
        .set_serving::<PublicServiceServer<MassaPublicGrpc>>()
        .await;
}

// Configure and start the gRPC API with the given service
async fn serve<S>(service: S, config: &GrpcConfig) -> Result<StopHandle, GrpcError>
where
    S: Service<Request<Body>, Response = Response<BoxBody>, Error = Infallible>
        + NamedService
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
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

    if config.enable_tls {
        if config.generate_self_signed_certificates {
            let (ca_cert, ca_key_pair) = mk_ca_cert().expect("error, failed to generate CA cert");
            let (cert, key_pair) =
                mk_ca_signed_cert(&ca_cert, &ca_key_pair).expect("error, failed to generate cert");

            // Convert the X509 certificates to PEM-encoded bytes
            let cert_pem_bytes = cert.to_pem().expect("Error: Failed to convert cert to PEM");
            let private_key_pem_bytes = key_pair
                .private_key_to_pem_pkcs8()
                .expect("Error: Failed to convert private key to PEM");

            if config.use_same_certificate_authority_for_client {
                let ca_cert_pem_bytes = ca_cert
                    .to_pem()
                    .expect("Error: Failed to convert CA cert to PEM");
                let ca_cert_pem_str = String::from_utf8(ca_cert_pem_bytes)
                    .expect("Error: Failed to convert CA cert to UTF-8");
                std::fs::write(
                    config.client_certificate_authority_root_path.clone(),
                    ca_cert_pem_str,
                )
                .expect("error, failed to write client certificate authority root");
            }

            let cert_pem_str =
                String::from_utf8(cert_pem_bytes).expect("Error: Failed to convert cert to UTF-8");
            std::fs::write(config.server_certificate_path.clone(), cert_pem_str)
                .expect("error, failed to write server certificat");

            let private_key_pem_str = String::from_utf8(private_key_pem_bytes)
                .expect("Error: Failed to convert private key to UTF-8");
            std::fs::write(config.server_private_key_path.clone(), private_key_pem_str)
                .expect("error, failed to write server private key");
        }

        let cert = std::fs::read_to_string(config.server_certificate_path.clone())
            .expect("error, failed to read server certificat");
        let key = std::fs::read_to_string(config.server_private_key_path.clone())
            .expect("error, failed to read server private key");
        let server_identity = Identity::from_pem(cert, key);

        let tls = ServerTlsConfig::new().identity(server_identity);

        if config.enable_mtls {
            let client_ca_cert =
                std::fs::read_to_string(config.client_certificate_authority_root_path.clone())
                    .expect("error, failed to read client certificate authority root");
            let client_ca_cert = Certificate::from_pem(client_ca_cert);

            let tls = tls.client_ca_root(client_ca_cert);

            server_builder = server_builder
                .tls_config(tls)
                .expect("error, failed to setup mTLS");

            info!("gRPC mTLS enabled");
        } else {
            server_builder = server_builder
                .tls_config(tls)
                .expect("error, failed to setup TLS");
            info!("gRPC TLS enabled");
        }
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
            .set_serving::<PublicServiceServer<MassaPublicGrpc>>()
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
                .add_service(service);

            tokio::spawn(
                router_with_http1.serve_with_shutdown(config.bind, shutdown_recv.map(drop)),
            );
        } else {
            let router_with_http1 = server_builder
                .accept_http1(true)
                .layer(GrpcWebLayer::new())
                .add_optional_service(reflection_service_opt)
                .add_optional_service(health_service_opt)
                .add_service(service);

            tokio::spawn(
                router_with_http1.serve_with_shutdown(config.bind, shutdown_recv.map(drop)),
            );
        }
    } else {
        let router = server_builder
            .add_optional_service(reflection_service_opt)
            .add_optional_service(health_service_opt)
            .add_service(service);

        tokio::spawn(router.serve_with_shutdown(config.bind, shutdown_recv.map(drop)));
    }

    Ok(StopHandle {
        stop_cmd_sender: shutdown_send,
    })
}

/// Make a CA certificate and private key
fn mk_ca_cert() -> Result<(X509, PKey<Private>), ErrorStack> {
    let rsa = Rsa::generate(2048)?;
    let key_pair = PKey::from_rsa(rsa)?;

    let mut x509_name = X509NameBuilder::new()?;
    x509_name.append_entry_by_text("CN", "localhost")?;
    x509_name.append_entry_by_text("O", "Massalabs")?;

    let x509_name = x509_name.build();

    let mut cert_builder = X509::builder()?;

    cert_builder.set_version(2)?;
    let serial_number = {
        let mut serial = BigNum::new()?;
        serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
        serial.to_asn1_integer()?
    };
    cert_builder.set_serial_number(&serial_number)?;
    cert_builder.set_subject_name(&x509_name)?;
    cert_builder.set_issuer_name(&x509_name)?;
    cert_builder.set_pubkey(&key_pair)?;
    let not_before = Asn1Time::days_from_now(0)?;
    cert_builder.set_not_before(&not_before)?;
    let not_after = Asn1Time::days_from_now(365)?;
    cert_builder.set_not_after(&not_after)?;
    cert_builder.append_extension(BasicConstraints::new().critical().ca().build()?)?;
    cert_builder.append_extension(
        KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()?,
    )?;

    let subject_key_identifier =
        SubjectKeyIdentifier::new().build(&cert_builder.x509v3_context(None, None))?;
    cert_builder.append_extension(subject_key_identifier)?;

    cert_builder.sign(&key_pair, MessageDigest::sha256())?;
    let cert = cert_builder.build();

    Ok((cert, key_pair))
}

/// Make a X509 request with the given private key
fn mk_request(key_pair: &PKey<Private>) -> Result<X509Req, ErrorStack> {
    let mut req_builder = X509ReqBuilder::new()?;
    req_builder.set_pubkey(key_pair)?;

    let mut x509_name = X509NameBuilder::new()?;
    x509_name.append_entry_by_text("CN", "localhost")?;
    x509_name.append_entry_by_text("O", "Massalabs")?;
    let x509_name = x509_name.build();
    req_builder.set_subject_name(&x509_name)?;

    req_builder.sign(key_pair, MessageDigest::sha256())?;
    let req = req_builder.build();
    Ok(req)
}

/// Make a certificate and private key signed by the given CA cert and private key
fn mk_ca_signed_cert(
    ca_cert: &X509Ref,
    ca_key_pair: &PKeyRef<Private>,
) -> Result<(X509, PKey<Private>), ErrorStack> {
    let rsa = Rsa::generate(2048)?;
    let key_pair = PKey::from_rsa(rsa)?;

    let req = mk_request(&key_pair)?;

    let mut cert_builder = X509::builder()?;
    cert_builder.set_version(2)?;
    let serial_number = {
        let mut serial = BigNum::new()?;
        serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
        serial.to_asn1_integer()?
    };
    cert_builder.set_serial_number(&serial_number)?;
    cert_builder.set_subject_name(req.subject_name())?;
    cert_builder.set_issuer_name(ca_cert.subject_name())?;
    cert_builder.set_pubkey(&key_pair)?;
    let not_before = Asn1Time::days_from_now(0)?;
    cert_builder.set_not_before(&not_before)?;
    let not_after = Asn1Time::days_from_now(365)?;
    cert_builder.set_not_after(&not_after)?;

    cert_builder.append_extension(BasicConstraints::new().build()?)?;

    cert_builder.append_extension(
        KeyUsage::new()
            .critical()
            .non_repudiation()
            .digital_signature()
            .key_encipherment()
            .build()?,
    )?;

    let subject_key_identifier =
        SubjectKeyIdentifier::new().build(&cert_builder.x509v3_context(Some(ca_cert), None))?;
    cert_builder.append_extension(subject_key_identifier)?;

    let auth_key_identifier = AuthorityKeyIdentifier::new()
        .keyid(false)
        .issuer(false)
        .build(&cert_builder.x509v3_context(Some(ca_cert), None))?;
    cert_builder.append_extension(auth_key_identifier)?;

    let subject_alt_name = SubjectAlternativeName::new()
        .dns("localhost")
        .build(&cert_builder.x509v3_context(Some(ca_cert), None))?;
    cert_builder.append_extension(subject_alt_name)?;

    cert_builder.sign(ca_key_pair, MessageDigest::sha256())?;
    let cert = cert_builder.build();

    Ok((cert, key_pair))
}
