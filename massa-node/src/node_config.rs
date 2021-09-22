// Copyright (c) 2021 MASSA LABS <info@massa.net>

use api::ApiConfig;
use bootstrap::config::BootstrapConfig;
use communication::network::NetworkConfig;
use communication::protocol::ProtocolConfig;
use consensus::ConsensusConfig;
use models::Version;
use pool::PoolConfig;
use rpc_server::APIConfig;
use serde::Deserialize;
use storage::StorageConfig;

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    pub level: usize,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub logging: LoggingConfig,
    pub protocol: ProtocolConfig,
    pub network: NetworkConfig,
    pub consensus: ConsensusConfig,
    pub api: ApiConfig,
    pub new_api: APIConfig,
    pub storage: StorageConfig,
    pub bootstrap: BootstrapConfig,
    pub pool: PoolConfig,
    pub version: Version,
}
