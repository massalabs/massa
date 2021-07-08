use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub default_node: SocketAddr,
}

impl Config {
    pub fn from_toml(toml_str: &str) -> Result<Config, toml::de::Error> {
        toml::de::from_str(toml_str)
    }
}
