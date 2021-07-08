use crypto::signature::PublicKey;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};
use time::UTime;

#[derive(Debug, Deserialize, Clone)]
pub struct BootstrapConfig {
    /// Ip address of our bootstrap node.
    pub bootstrap_ip: IpAddr,
    /// Port where to connect for bootstrapping.
    pub bootstrap_port: u16,
    /// Bootstrap node's public key.
    pub bootstrap_public_key: PublicKey,
    /// Port to listen if we choose to allow other nodes to use us as bootstrap node.
    pub bind: Option<SocketAddr>,
    /// connection timeout
    pub connect_timeout: UTime,
    /// If we are before genesis_time + bootstrap_time_after_genesis we try to bootstrap else we don't
    pub bootstrap_time_after_genesis: UTime,
    /// Time we wait before retrying a bootstrap
    pub retry_delay: UTime,
}
