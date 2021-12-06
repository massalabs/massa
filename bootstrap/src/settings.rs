// Copyright (c) 2021 MASSA LABS <info@massa.net>

use serde::Deserialize;
use signature::PublicKey;
use std::net::SocketAddr;
use time::UTime;

#[derive(Debug, Deserialize, Clone)]
pub struct BootstrapSettings {
    /// Ip address of our bootstrap nodes and their public key.
    pub bootstrap_list: Vec<(SocketAddr, PublicKey)>,
    /// Port to listen if we choose to allow other nodes to use us as bootstrap node.
    pub bind: Option<SocketAddr>,
    /// connection timeout
    pub connect_timeout: UTime,
    /// readout timeout
    pub read_timeout: UTime,
    /// write timeout
    pub write_timeout: UTime,
    /// Time we wait before retrying a bootstrap
    pub retry_delay: UTime,
    /// Max message size for bootstrap
    pub max_bootstrap_message_size: u32,
    /// Max number of blocks we provide/ take into account while bootstrapping
    pub max_bootstrap_blocks: u32,
    pub max_bootstrap_cliques: u32,
    pub max_bootstrap_deps: u32,
    pub max_bootstrap_children: u32,
    /// Max ping delay.
    pub max_ping: UTime,
    /// Max number of cycles in PoS bootstrap
    pub max_bootstrap_pos_cycles: u32,
    /// Max number of address and rng entries for PoS bootstrap
    pub max_bootstrap_pos_entries: u32,
    /// Enable clock synchronization
    pub enable_clock_synchronization: bool,
    // Cache duration
    pub cache_duration: UTime,
    // Max simultaneous bootstraps
    pub max_simultaneous_bootstraps: u32,
    // Minimum interval between two bootstrap attempts from a given IP
    pub per_ip_min_interval: UTime,
    // Max size of the IP list
    pub ip_list_max_size: usize,
}
