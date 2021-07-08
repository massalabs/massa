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
    /// Time we wait before retrying a bootstrap
    pub retry_delay: UTime,
    /// Max number of blocks we provide/ take into account while bootstrapping
    pub max_bootstrap_blocks: u32,
    pub max_bootstrap_cliques: u32,
    pub max_bootstrap_deps: u32,
    pub max_bootstrap_children: u32,
    /// Max ping delay.
    pub max_ping: UTime,
}
