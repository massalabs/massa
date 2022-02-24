// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_time::MassaTime;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};

/// Network configuration
#[derive(Debug, Deserialize, Clone)]
pub struct NetworkSettings {
    /// Where to listen for communications.
    pub bind: SocketAddr,
    /// Our own IP if it is routable, else None.
    pub routable_ip: Option<IpAddr>,
    /// Protocol port
    pub protocol_port: u16,
    /// Time interval spent waiting for a response from a peer.
    /// In millis
    pub connect_timeout: MassaTime,
    /// Network_worker will try to connect to available peers every wakeup_interval.
    /// In millis
    pub wakeup_interval: MassaTime,
    /// Path to the file containing initial peers.
    pub initial_peers_file: std::path::PathBuf,
    /// Path to the file containing known peers.
    pub peers_file: std::path::PathBuf,
    /// Path to the file containing our private_key
    pub private_key_file: std::path::PathBuf,
    /// Target number of bootstrap connections.
    pub target_bootstrap_connections: usize,
    /// Limit on the number of simultaneout outgoing bootstrap connection attempts.
    pub max_out_bootstrap_connection_attempts: usize,
    /// Target number of outgoing nonbootstrap connections.
    pub target_out_nonbootstrap_connections: usize,
    /// Limit on the number of in connections.
    pub max_in_nonbootstrap_connections: usize,
    /// Limit on the number of in connections per ip.
    pub max_in_connections_per_ip: usize,
    /// Limit on the total current number of outgoing non-bootstrap connection attempts.
    pub max_out_nonbootstrap_connection_attempts: usize,
    /// Limit on the number of idle peers we remember.
    pub max_idle_peers: usize,
    /// Limit on the number of banned peers we remember.
    pub max_banned_peers: usize,
    /// Peer database is dumped every peers_file_dump_interval in millis
    pub peers_file_dump_interval: MassaTime,
    /// After message_timeout millis we are no longer waiting on handshake message
    pub message_timeout: MassaTime,
    /// Every ask_peer_list_interval in millis we ask every one for its advertisable peers list.
    pub ask_peer_list_interval: MassaTime,
    /// Max wait time for sending a Network or Node event.
    pub max_send_wait: MassaTime,
    /// Time after which we forget a node
    pub ban_timeout: MassaTime,
    /// Timeout Duration when we send a PeerList in handshake
    pub peer_list_send_timeout: MassaTime,
    /// Max number of in connection overflowed managed by the handshake that send a list of peers
    pub max_in_connection_overflow: usize,
}

#[cfg(test)]
mod tests {
    use crate::NetworkSettings;
    use massa_models::constants::*;
    use massa_time::MassaTime;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    impl Default for NetworkSettings {
        fn default() -> Self {
            NetworkSettings {
                bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
                routable_ip: Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))),
                protocol_port: 0,
                connect_timeout: MassaTime::from(180_000),
                wakeup_interval: MassaTime::from(10_000),
                peers_file: std::path::PathBuf::new(),
                target_bootstrap_connections: 1,
                max_out_bootstrap_connection_attempts: 1,
                target_out_nonbootstrap_connections: 10,
                max_in_nonbootstrap_connections: 5,
                max_in_connections_per_ip: 2,
                max_out_nonbootstrap_connection_attempts: 15,
                max_idle_peers: 3,
                max_banned_peers: 3,
                peers_file_dump_interval: MassaTime::from(10_000),
                message_timeout: MassaTime::from(5000u64),
                ask_peer_list_interval: MassaTime::from(50000u64),
                private_key_file: std::path::PathBuf::new(),
                max_send_wait: MassaTime::from(100),
                ban_timeout: MassaTime::from(100_000_000),
                initial_peers_file: std::path::PathBuf::new(),
                peer_list_send_timeout: MassaTime::from(500),
                max_in_connection_overflow: 2,
            }
        }
    }

    impl NetworkSettings {
        pub fn scenarios_default(port: u16, peers_file: &std::path::Path) -> Self {
            // Init the serialization context with a default,
            // can be overwritten with a more specific one in the test.
            massa_models::init_serialization_context(massa_models::SerializationContext {
                max_advertise_length: 128,
                max_bootstrap_blocks: 100,
                max_bootstrap_cliques: 100,
                max_bootstrap_deps: 100,
                max_bootstrap_children: 100,
                max_ask_blocks_per_message: 10,
                endorsement_count: 8,
                ..massa_models::SerializationContext::default()
            });
            Self {
                bind: format!("0.0.0.0:{}", port).parse().unwrap(),
                routable_ip: Some(BASE_NETWORK_CONTROLLER_IP),
                protocol_port: port,
                connect_timeout: MassaTime::from(3000),
                peers_file: peers_file.to_path_buf(),
                target_out_nonbootstrap_connections: 10,
                wakeup_interval: MassaTime::from(3000),
                target_bootstrap_connections: 0,
                max_out_bootstrap_connection_attempts: 1,
                max_in_nonbootstrap_connections: 100,
                max_in_connections_per_ip: 100,
                max_out_nonbootstrap_connection_attempts: 100,
                max_idle_peers: 100,
                max_banned_peers: 100,
                peers_file_dump_interval: MassaTime::from(30000),
                message_timeout: MassaTime::from(5000u64),
                ask_peer_list_interval: MassaTime::from(50000u64),
                private_key_file: crate::tests::tools::get_temp_private_key_file()
                    .path()
                    .to_path_buf(),
                max_send_wait: MassaTime::from(100),
                ban_timeout: MassaTime::from(100_000_000),
                initial_peers_file: peers_file.to_path_buf(),
                peer_list_send_timeout: MassaTime::from(50),
                max_in_connection_overflow: 10,
            }
        }
    }
}
