use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Deserialize, Clone)]
pub struct APIConfig {
    pub draw_lookahead_period_count: u64,
    pub bind_private: SocketAddr,
    pub bind_public: SocketAddr,
}
