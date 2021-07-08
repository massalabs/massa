use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};

#[derive(Debug, Deserialize, Clone)]
pub struct NetworkConfig {
    pub bind: SocketAddr,
    pub routable_ip: Option<IpAddr>,
    pub protocol_port: u16,
    pub connect_timeout_seconds: f32,
    pub peers_file: std::path::PathBuf,
    pub target_out_connections: usize,
    pub max_in_connections: usize,
    pub max_in_connections_per_ip: usize,
    pub max_out_connnection_attempts: usize,
    pub max_idle_peers: usize,
    pub max_banned_peers: usize,
    pub max_advertise_length: usize,
    pub peers_file_dump_interval_seconds: f32,
}
