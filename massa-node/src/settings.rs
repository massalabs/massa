// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Build here the default node settings from the config file toml
use std::path::PathBuf;

use massa_bootstrap::settings::BootstrapSettings;
use massa_consensus_exports::ConsensusSettings;
use massa_models::{
    api::APISettings,
    constants::{build_massa_settings, OPERATION_VALIDITY_PERIODS, THREAD_COUNT},
};
use massa_network::NetworkSettings;
use massa_pool::{PoolConfig, PoolSettings};
use massa_protocol_exports::ProtocolSettings;
use massa_time::MassaTime;
use serde::Deserialize;

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
    pub final_history_length: usize,
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
