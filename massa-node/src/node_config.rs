// Copyright (c) 2021 MASSA LABS <info@massa.net>

use bootstrap::config::BootstrapConfig;
use consensus::ConsensusConfig;
use models::api::APIConfig;
use models::Version;
use network::NetworkConfig;
use pool::PoolConfig;
use protocol_exports::ProtocolConfig;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct LoggingConfig {
    pub level: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub logging: LoggingConfig,
    pub protocol: ProtocolConfig,
    pub network: NetworkConfig,
    pub consensus: ConsensusConfig,
    pub api: APIConfig,
    pub bootstrap: BootstrapConfig,
    pub pool: PoolConfig,
    pub version: Version,
}
