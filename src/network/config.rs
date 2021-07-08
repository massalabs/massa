use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Deserialize, Clone)]
pub struct NetworkConfig {
    pub bind: SocketAddr,
    pub known_peers_file: String,
    pub retry_wait_seconds: f32,
    pub timeout_seconds: f32,
    pub target_outgoing_connections: usize,
    pub max_incoming_connections: usize,
    pub max_simultaneous_outgoing_connection_attempts: usize,
    pub max_simultaneous_incoming_connection_attempts: usize,
    pub max_known_nodes: usize,
    pub peer_file_dump_interval_seconds: f32,
}
