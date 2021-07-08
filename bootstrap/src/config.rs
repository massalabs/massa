use crypto::signature::PublicKey;
use serde::Deserialize;
use std::net::SocketAddr;
use time::UTime;

#[derive(Debug, Deserialize, Clone)]
pub struct BootstrapConfig {
    /// Ip address of our bootstrap node, if we are to bootstrap.
    pub bootstrap_addr: Option<SocketAddr>,
    /// Bootstrap node's public key.
    pub bootstrap_public_key: PublicKey,
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
}
