// Copyright (c) 2025 MASSA LABS <info@massa.net>
//! Integration tests for the TLS/mTLS gRPC channel of the SDK, against a real
//! TLS-enabled tonic server using certificates from `cert_manager` (the same
//! generation path the node uses for its self-signed certificates).

use massa_sdk::cert_manager::{gen_cert_for_ca, gen_signed_cert};
use massa_sdk::{connect_grpc_channel, ClientError, GrpcTlsConfig};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};

struct TlsMaterial {
    _dir: tempfile::TempDir,
    ca_path: PathBuf,
    other_ca_path: PathBuf,
    client_cert_path: PathBuf,
    client_key_path: PathBuf,
    server_identity: Identity,
    ca_pem: String,
}

/// Generate a CA, a server certificate (subject alt name "localhost"), a client
/// certificate signed by the same CA, and an unrelated CA, all written to disk.
fn generate_tls_material() -> TlsMaterial {
    let dir = tempfile::tempdir().unwrap();
    let ca = gen_cert_for_ca().unwrap();
    let ca_pem = ca.serialize_pem().unwrap();
    let (server_cert_pem, server_key_pem) = gen_signed_cert(&ca, vec![]).unwrap();
    let (client_cert_pem, client_key_pem) = gen_signed_cert(&ca, vec![]).unwrap();
    let other_ca_pem = gen_cert_for_ca().unwrap().serialize_pem().unwrap();

    let ca_path = dir.path().join("ca.pem");
    let other_ca_path = dir.path().join("other_ca.pem");
    let client_cert_path = dir.path().join("client.pem");
    let client_key_path = dir.path().join("client.key");
    std::fs::write(&ca_path, &ca_pem).unwrap();
    std::fs::write(&other_ca_path, other_ca_pem).unwrap();
    std::fs::write(&client_cert_path, client_cert_pem).unwrap();
    std::fs::write(&client_key_path, client_key_pem).unwrap();

    TlsMaterial {
        _dir: dir,
        ca_path,
        other_ca_path,
        client_cert_path,
        client_key_path,
        server_identity: Identity::from_pem(server_cert_pem, server_key_pem),
        ca_pem,
    }
}

/// Spawn a gRPC health server on an ephemeral port, with the given TLS setup.
async fn spawn_server(tls: Option<ServerTlsConfig>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_service_status("", tonic_health::ServingStatus::Serving)
        .await;
    let mut builder = Server::builder();
    if let Some(tls) = tls {
        builder = builder.tls_config(tls).unwrap();
    }
    tokio::spawn(
        builder
            .add_service(health_service)
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );
    addr
}

/// Perform a health-check RPC over the given channel.
async fn health_check(
    channel: tonic::transport::Channel,
) -> Result<tonic::Response<tonic_health::pb::HealthCheckResponse>, tonic::Status> {
    tonic_health::pb::health_client::HealthClient::new(channel)
        .check(tonic_health::pb::HealthCheckRequest {
            service: "".to_string(),
        })
        .await
}

fn tls_config(material: &TlsMaterial, with_identity: bool) -> GrpcTlsConfig {
    GrpcTlsConfig {
        server_name: "localhost".to_string(),
        certificate_authority_root_path: material.ca_path.clone(),
        client_certificate_path: with_identity.then(|| material.client_cert_path.clone()),
        client_private_key_path: with_identity.then(|| material.client_key_path.clone()),
    }
}

#[tokio::test]
async fn test_plaintext_channel_still_connects() {
    let addr = spawn_server(None).await;
    connect_grpc_channel(addr, None).await.unwrap();
}

#[tokio::test]
async fn test_tls_channel_connects() {
    let material = generate_tls_material();
    let addr = spawn_server(Some(
        ServerTlsConfig::new().identity(material.server_identity.clone()),
    ))
    .await;
    connect_grpc_channel(addr, Some(&tls_config(&material, false)))
        .await
        .unwrap();
}

#[tokio::test]
async fn test_tls_channel_rejects_untrusted_server() {
    let material = generate_tls_material();
    let addr = spawn_server(Some(
        ServerTlsConfig::new().identity(material.server_identity.clone()),
    ))
    .await;
    // trust an unrelated CA: the server certificate must be rejected
    let mut config = tls_config(&material, false);
    config.certificate_authority_root_path = material.other_ca_path.clone();
    let err = connect_grpc_channel(addr, Some(&config)).await.unwrap_err();
    assert!(matches!(err, ClientError::Connect(_)));
}

#[tokio::test]
async fn test_mtls_channel_connects_with_identity() {
    let material = generate_tls_material();
    let addr = spawn_server(Some(
        ServerTlsConfig::new()
            .identity(material.server_identity.clone())
            .client_ca_root(Certificate::from_pem(material.ca_pem.clone())),
    ))
    .await;
    let channel = connect_grpc_channel(addr, Some(&tls_config(&material, true)))
        .await
        .unwrap();
    // the server only accepts authenticated clients, so a successful RPC proves
    // the client identity was presented and accepted
    health_check(channel).await.unwrap();
}

#[tokio::test]
async fn test_mtls_channel_rejects_client_without_identity() {
    let material = generate_tls_material();
    let addr = spawn_server(Some(
        ServerTlsConfig::new()
            .identity(material.server_identity.clone())
            .client_ca_root(Certificate::from_pem(material.ca_pem.clone())),
    ))
    .await;
    // the missing client certificate is only rejected by the server after the
    // handshake, so `connect` can succeed; the RPC itself must fail
    match connect_grpc_channel(addr, Some(&tls_config(&material, false))).await {
        Err(err) => assert!(matches!(err, ClientError::Connect(_))),
        Ok(channel) => {
            health_check(channel).await.unwrap_err();
        }
    }
}

#[tokio::test]
async fn test_incomplete_client_identity_is_a_configuration_error() {
    let material = generate_tls_material();
    let mut config = tls_config(&material, true);
    config.client_private_key_path = None;
    // no server needed: the configuration is rejected before dialing
    let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
    let err = connect_grpc_channel(addr, Some(&config)).await.unwrap_err();
    assert!(matches!(err, ClientError::InvalidTlsConfig(_)));
}
