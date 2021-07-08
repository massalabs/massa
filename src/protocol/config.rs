use crate::network::config::NetworkConfig;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct ProtocolConfig {
    pub network: NetworkConfig,
}
