use serde::Deserialize;

pub const CHANNEL_SIZE: usize = 256;

/// Pool configuration
#[derive(Debug, Deserialize, Clone)]
pub struct PoolConfig {
}
