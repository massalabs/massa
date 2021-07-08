use serde::Deserialize;
use time::UTime;

/// Protocol Configuration
#[derive(Debug, Deserialize, Clone)]
pub struct ProtocolConfig {
    /// After message_timeout millis we are no longer waiting on handshake message
    pub message_timeout: UTime,
    /// Every ask_peer_list_interval in millis we ask every one for its advertisable peers list.
    pub ask_peer_list_interval: UTime,
}
