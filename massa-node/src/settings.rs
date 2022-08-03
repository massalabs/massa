// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Build here the default node settings from the configuration file toml
use std::path::PathBuf;

use enum_map::EnumMap;
use massa_api::APISettings;
use massa_bootstrap::BootstrapSettings;
use massa_consensus_exports::ConsensusSettings;
use massa_models::constants::{build_massa_settings, OPERATION_VALIDITY_PERIODS, THREAD_COUNT};
use massa_pool::{PoolConfig, PoolSettings};
use massa_protocol_exports::ProtocolSettings;
use massa_time::MassaTime;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};

use massa_network_exports::{settings::PeerTypeConnectionConfig, PeerType};

lazy_static::lazy_static! {
    pub static ref SETTINGS: Settings = build_massa_settings("massa-node", "MASSA_NODE");
    pub static ref POOL_CONFIG: PoolConfig = PoolConfig {
        settings: SETTINGS.pool,
        thread_count: THREAD_COUNT,
        operation_validity_periods: OPERATION_VALIDITY_PERIODS
    };
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingSettings {
    pub level: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExecutionSettings {
    pub max_final_events: usize,
    pub readonly_queue_length: usize,
    pub cursor_delay: MassaTime,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LedgerSettings {
    pub initial_sce_ledger_path: PathBuf,
    pub disk_ledger_path: PathBuf,
    pub final_history_length: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NetworkSettings {
    pub bind: SocketAddr,
    pub routable_ip: Option<IpAddr>,
    pub protocol_port: u16,
    pub connect_timeout: MassaTime,
    pub wakeup_interval: MassaTime,
    pub initial_peers_file: std::path::PathBuf,
    pub peers_file: std::path::PathBuf,
    pub keypair_file: std::path::PathBuf,
    pub peer_types_config: EnumMap<PeerType, PeerTypeConnectionConfig>,
    pub max_in_connections_per_ip: usize,
    pub max_idle_peers: usize,
    pub max_banned_peers: usize,
    pub peers_file_dump_interval: MassaTime,
    pub message_timeout: MassaTime,
    pub ask_peer_list_interval: MassaTime,
    pub max_send_wait: MassaTime,
    pub ban_timeout: MassaTime,
    pub peer_list_send_timeout: MassaTime,
    pub max_in_connection_overflow: usize,
    pub max_operations_per_message: u32,
    pub max_bytes_read: f64,
    pub max_bytes_write: f64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Settings {
    pub logging: LoggingSettings,
    pub protocol: ProtocolSettings,
    pub network: NetworkSettings,
    pub consensus: ConsensusSettings,
    pub api: APISettings,
    pub bootstrap: BootstrapSettings,
    pub pool: PoolSettings,
    pub execution: ExecutionSettings,
    pub ledger: LedgerSettings,
}

#[cfg(test)]
#[test]
fn test_load_node_config() {
    let _ = *SETTINGS;
}
