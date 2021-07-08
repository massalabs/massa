use serde::Deserialize;
use std::net::SocketAddr;

///
#[derive(Debug, Deserialize, Clone)]
pub struct ApiConfig {
    /// limit on the number of invalid blocks that are returned, to avoid flooding
    pub max_return_invalid_blocks: usize,
    /// how many periods should be considered while retrieving staker's next slots
    pub selection_return_periods: u64,
    /// where is the api listening
    pub bind: SocketAddr,
}
