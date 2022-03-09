// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_signature::PublicKey;
use massa_time::MassaTime;
use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Deserialize, Clone)]
pub struct BootstrapSettings {
    /// Ip address of our bootstrap nodes and their public key.
    pub bootstrap_list: Vec<(SocketAddr, PublicKey)>,
    /// Port to listen if we choose to allow other nodes to use us as bootstrap node.
    pub bind: Option<SocketAddr>,
    /// connection timeout
    pub connect_timeout: MassaTime,
    /// readout timeout
    pub read_timeout: MassaTime,
    /// write timeout
    pub write_timeout: MassaTime,
    /// Time we wait before retrying a bootstrap
    pub retry_delay: MassaTime,
    /// Max ping delay.
    pub max_ping: MassaTime,
    /// Enable clock synchronization
    pub enable_clock_synchronization: bool,
    /// Cache duration
    pub cache_duration: MassaTime,
    /// Max simultaneous bootstraps
    pub max_simultaneous_bootstraps: u32,
    /// Minimum interval between two bootstrap attempts from a given IP
    pub per_ip_min_interval: MassaTime,
    /// Max size of the IP list
    pub ip_list_max_size: usize,
}
