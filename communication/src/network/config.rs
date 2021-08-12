// Copyright (c) 2021 MASSA LABS <info@massa.net>

use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};
use time::UTime;

pub const CHANNEL_SIZE: usize = 256;

/// Network configuration
#[derive(Debug, Deserialize, Clone)]
pub struct NetworkConfig {
    /// Where to listen for communications.
    pub bind: SocketAddr,
    /// Our own IP if it is routable, else None.
    pub routable_ip: Option<IpAddr>,
    /// Protocol port
    pub protocol_port: u16,
    /// Time interval spent waiting for a response from a peer.
    /// In millis
    pub connect_timeout: UTime,
    /// Network_worker will try to connect to available peers every wakeup_interval.
    /// In millis
    pub wakeup_interval: UTime,
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
    /// Limit on the number of peers we advertise to others.
    pub max_advertise_length: u32,
    /// Peer database is dumped every peers_file_dump_interval in millis
    pub peers_file_dump_interval: UTime,
    /// Maximum message length in bytes
    pub max_message_size: u32,
    /// After message_timeout millis we are no longer waiting on handshake message
    pub message_timeout: UTime,
    /// Every ask_peer_list_interval in millis we ask every one for its advertisable peers list.
    pub ask_peer_list_interval: UTime,
    /// Max number of hash in the message AskForBlocks
    pub max_ask_blocks_per_message: u32,
    /// Max number of operations per message
    pub max_operations_per_message: u32,
    /// Max number of endorsements per message
    pub max_endorsements_per_message: u32,
    /// Max wait time for sending a Network or Node event.
    pub max_send_wait: UTime,
    /// Time after which we forget a node
    pub ban_timeout: UTime,
}
