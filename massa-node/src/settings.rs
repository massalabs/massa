// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Build here the default node settings from the config file toml
use massa_bootstrap::settings::BootstrapSettings;
use massa_consensus_exports::ConsensusSettings;
use massa_execution::ExecutionSettings;
use massa_models::{
    api::APISettings,
    constants::{build_massa_settings, OPERATION_VALIDITY_PERIODS, THREAD_COUNT},
};
use massa_network_exports::NetworkSettings;
use massa_pool::{PoolConfig, PoolSettings};
use massa_protocol_exports::ProtocolSettings;
use serde::Deserialize;

lazy_static::lazy_static! {
    pub static ref SETTINGS: Settings = build_massa_settings("massa-node", "MASSA_NODE");
    pub static ref POOL_CONFIG: PoolConfig = PoolConfig {
        settings: SETTINGS.pool,
        thread_count: THREAD_COUNT,
        operation_validity_periods: OPERATION_VALIDITY_PERIODS
    };
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct LoggingSettings {
    pub level: usize,
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
}
