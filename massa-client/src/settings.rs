// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Build here the default client settings from the configuration file toml
use massa_models::config::build_massa_settings;
use massa_time::MassaTime;
use serde::Deserialize;
use std::{net::IpAddr, path::PathBuf};

lazy_static::lazy_static! {
    pub static ref SETTINGS: Settings = build_massa_settings("massa-client", "MASSA_CLIENT");
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub default_node: DefaultNode,
    pub history: usize,
    pub history_file_path: PathBuf,
    pub timeout: MassaTime,
    pub client: ClientSettings,
    pub cli: CliSettings,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CliSettings {
    pub approved_community_charter_file_path: PathBuf,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DefaultNode {
    pub ip: IpAddr,
    pub private_port: u16,
    pub public_port: u16,
    pub grpc_public_port: u16,
    pub grpc_private_port: u16,
    pub chain_id: Option<u64>,
}

/// Client settings
/// the client settings.
#[derive(Debug, Deserialize, Clone)]
pub struct ClientSettings {
    pub max_request_body_size: u32,
    pub request_timeout: MassaTime,
    pub max_concurrent_requests: usize,
    pub certificate_store: String,
    pub id_kind: String,
    pub max_log_length: u32,
    pub headers: Vec<(String, String)>,
    pub http: HttpSettings,
    pub grpc: GrpcSettings,
}

///TODO add WebSocket to CLI
/// Http client settings.
/// the Http client settings
#[derive(Debug, Deserialize, Clone)]
pub struct HttpSettings {
    pub enabled: bool,
}

/// gRPC client settings, one TLS block per endpoint.
#[derive(Debug, Deserialize, Clone)]
pub struct GrpcSettings {
    pub public: GrpcTlsSettings,
    pub private: GrpcTlsSettings,
}

/// TLS settings towards one gRPC endpoint of the node.
#[derive(Debug, Deserialize, Clone)]
pub struct GrpcTlsSettings {
    pub enable_tls: bool,
    pub enable_mtls: bool,
    pub server_name: String,
    pub certificate_authority_root_path: PathBuf,
    pub client_certificate_path: PathBuf,
    pub client_private_key_path: PathBuf,
}

#[cfg(test)]
#[test]
fn test_load_client_config() {
    let _ = *SETTINGS;
}
