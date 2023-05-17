// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Build here the default client settings from the configuration file toml
use massa_models::config::build_massa_settings;
use massa_time::MassaTime;
use serde::Deserialize;
use std::{net::IpAddr, path::PathBuf};

lazy_static::lazy_static! {
    pub(crate)  static ref SETTINGS: Settings = build_massa_settings("massa-client", "MASSA_CLIENT");
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct Settings {
    pub(crate) default_node: DefaultNode,
    pub(crate) history: usize,
    pub(crate) history_file_path: PathBuf,
    pub(crate) timeout: MassaTime,
    pub(crate) client: ClientSettings,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DefaultNode {
    pub(crate) ip: IpAddr,
    pub(crate) private_port: u16,
    pub(crate) public_port: u16,
    pub(crate) grpc_port: u16,
}

/// Client settings
/// the client settings.
#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ClientSettings {
    pub(crate) max_request_body_size: u32,
    pub(crate) request_timeout: MassaTime,
    pub(crate) max_concurrent_requests: usize,
    pub(crate) certificate_store: String,
    pub(crate) id_kind: String,
    pub(crate) max_log_length: u32,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) http: HttpSettings,
}

///TODO add WebSocket to CLI
/// Http client settings.
/// the Http client settings
#[derive(Debug, Deserialize, Clone)]
pub(crate) struct HttpSettings {
    pub(crate) enabled: bool,
}

#[cfg(test)]
#[test]
fn test_load_client_config() {
    let _ = *SETTINGS;
}
