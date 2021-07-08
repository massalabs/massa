use serde::Deserialize;
use toml;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub logging: LoggingConfig,
    pub network: NetworkConfig,
}

#[derive(Debug, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
}

#[derive(Debug, Deserialize)]
pub struct NetworkConfig {
    pub bind: String,
    pub node_key_file: String,
}

impl Config {
    pub fn from_toml(toml_str: &String) -> Result<Config, toml::de::Error> {
        toml::de::from_str(toml_str)
    }
}
