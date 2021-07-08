use serde::Deserialize;
use time::UTime;

#[derive(Debug, Deserialize, Clone)]
pub struct ProtocolConfig {
    pub message_timeout: UTime,
    pub ask_peer_list_interval: UTime,
}
