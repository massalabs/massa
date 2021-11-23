// Copyright (c) 2021 MASSA LABS <info@massa.net>

use bootstrap::settings::BootstrapSettings;
use consensus::ConsensusConfig;
use models::api::APISettings;
use models::Version;
use network::NetworkConfig;
use pool::PoolSettings;
use protocol_exports::ProtocolSettings;
use serde::Deserialize;

const BASE_CONFIG_PATH: &str = "base_config/config.toml";
const OVERRIDE_CONFIG_PATH: &str = "config/config.toml";

lazy_static::lazy_static! {
    pub static ref SETTINGS: Settings = {
        let mut settings = config::Config::default();
        settings
            .merge(config::File::with_name(BASE_CONFIG_PATH))
            .unwrap();
        if std::path::Path::new(OVERRIDE_CONFIG_PATH).is_file() {
            settings
                .merge(config::File::with_name(OVERRIDE_CONFIG_PATH))
                .unwrap();
        }
        settings
            .merge(config::Environment::with_prefix("MASSA_CLIENT"))
            .unwrap();
        settings.try_into().unwrap()
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
    pub network: NetworkConfig,
    pub consensus: ConsensusConfig,
    pub api: APISettings,
    pub bootstrap: BootstrapSettings,
    pub pool: PoolSettings,
    pub version: Version,
}
