use communication::network::config::NetworkConfig;
use communication::protocol::config::ProtocolConfig;
use consensus::config::ConsensusConfig;
use serde::Deserialize;
use toml;

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
}

impl Config {
    pub fn from_toml(toml_str: &str) -> Result<Config, toml::de::Error> {
        toml::de::from_str(toml_str)
    }
}
