use serde::Deserialize;
use crate::network::config::NetworkConfig;

#[derive(Debug, Deserialize, Clone)]
pub struct ProtocolConfig {
    network_config: NetworkConfig,
}
