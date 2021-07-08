use serde::Deserialize;
use time::UTime;

pub const CHANNEL_SIZE: usize = 16;

/// Protocol Configuration
#[derive(Debug, Deserialize, Clone)]
pub struct ProtocolConfig {
    pub ask_block_timeout: UTime,
}
