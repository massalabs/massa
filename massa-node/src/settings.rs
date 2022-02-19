// Copyright (c) 2021 MASSA LABS <info@massa.net>

//! Build here the default node settings from the config file toml
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

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct LoggingSettings {
    pub level: usize,
}

#[derive(Clone, Debug)]
pub struct ExecutionSettings {
    max_final_events: usize,
    readonly_queue_length: usize,
    cursor_delay: MassaTime,
}

#[derive(Clone, Debug)]
pub struct LedgerSettings {
    initial_sce_ledger_path: usize,
    final_history_length: usize,
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
