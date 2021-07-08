use serde::Deserialize;

pub const CHANNEL_SIZE: usize = 16;

/// Protocol Configuration
#[derive(Debug, Deserialize, Clone)]
pub struct ProtocolConfig {}
