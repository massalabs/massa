use crate::settings::PeerTypeConnectionConfig;
use displaydoc::Display;
use enum_map::Enum;
use massa_models::{
    node::NodeId, with_serialization_context, DeserializeCompact, DeserializeVarInt, ModelsError,
    SerializeCompact, SerializeVarInt,
};
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::IpAddr};

/// Associate a peer info with nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    /// peer info
    pub peer_info: PeerInfo,
    /// corresponding nodes (true if active)
    pub active_nodes: Vec<(NodeId, bool)>,
}

/// peers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peers {
    /// our node id
    pub our_node_id: NodeId,
    /// peers
    pub peers: HashMap<IpAddr, Peer>,
}

/// Peers that are transmitted during bootstrap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapPeers(pub Vec<IpAddr>);

impl SerializeCompact for BootstrapPeers {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // peers
        let peers_count: u32 = self.0.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many peers blocks in BootstrapPeers: {}", err))
        })?;
        let max_peer_list_length =
            with_serialization_context(|context| context.max_advertise_length);
        if peers_count > max_peer_list_length {
            return Err(ModelsError::SerializeError(format!(
                "too many peers for serialization context in BootstrapPeers: {}",
                peers_count
            )));
        }
        res.extend(peers_count.to_varint_bytes());
        for peer in self.0.iter() {
            res.extend(peer.to_bytes_compact()?);
        }

        Ok(res)
    }
}

impl DeserializeCompact for BootstrapPeers {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // peers
        let (peers_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        let max_peer_list_length =
            with_serialization_context(|context| context.max_advertise_length);
        if peers_count > max_peer_list_length {
            return Err(ModelsError::DeserializeError(format!(
                "too many peers for deserialization context in BootstrapPeers: {}",
                peers_count
            )));
        }
        cursor += delta;
        let mut peers: Vec<IpAddr> = Vec::with_capacity(peers_count as usize);
        for _ in 0..(peers_count as usize) {
            let (ip, delta) = IpAddr::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            peers.push(ip);
        }

        Ok((BootstrapPeers(peers), cursor))
    }
}

/// Peer categories.
/// There is a defined number of slots for each category.
/// Order matters: less prioritized peer type first
#[derive(
    Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Enum,
)]
pub enum PeerType {
    /// Just a peer
    Standard,
    /// Connection from these nodes are always accepted
    WhiteListed,
    /** if the peer is in bootstrap servers list
    for now it is decoupled from the real bootstrap sever list, it's just parsed
    TODO: `https://github.com/massalabs/massa/issues/2320`
    */
    Bootstrap,
}

mod test {

    #[test]
    fn test_order() {
        use crate::peers::PeerType;
        assert!(PeerType::Bootstrap > PeerType::WhiteListed);
        assert!(PeerType::WhiteListed > PeerType::Standard);
    }
}

impl Default for PeerType {
    fn default() -> Self {
        PeerType::Standard
    }
}

/// All information concerning a peer is here
#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct PeerInfo {
    /// Peer ip address.
    pub ip: IpAddr,
    /// The category the peer is in affects how it's treated.
    pub peer_type: PeerType,
    /// Time in milliseconds when peer was last alive
    pub last_alive: Option<MassaTime>,
    /// Time in milliseconds of peer's last failure
    pub last_failure: Option<MassaTime>,
    /// Whether peer was promoted through another peer
    pub advertised: bool,
    /// peer was banned
    pub banned: bool,
    /// Current number of active out connection attempts with that peer.
    /// Isn't dump into peer file.
    #[serde(default = "usize::default")]
    pub active_out_connection_attempts: usize,
    /// Current number of active out connections with that peer.
    /// Isn't dump into peer file.
    #[serde(default = "usize::default")]
    pub active_out_connections: usize,
    /// Current number of active in connections with that peer.
    /// Isn't dump into peer file.
    #[serde(default = "usize::default")]
    pub active_in_connections: usize,
}

impl PeerInfo {
    /// Cleans up the `PeerInfo` by normalizing the IP address
    /// and resetting active connection counts.
    pub fn cleanup(&mut self) {
        // canonicalize IP
        self.ip = self.ip.to_canonical();
        // ensure that connections are set to zero
        self.active_out_connection_attempts = 0;
        self.active_out_connections = 0;
        self.active_in_connections = 0;
    }

    /// Returns true if there is at least one connection attempt /
    /// one active connection in either direction
    /// with this peer
    #[inline]
    pub fn is_active(&self) -> bool {
        self.active_out_connection_attempts > 0
            || self.active_out_connections > 0
            || self.active_in_connections > 0
    }

    /// New standard `PeerInfo` for `IpAddr`
    ///
    /// # Arguments
    /// * `ip`: the IP address of the peer
    /// * `advertised`: true if this peer was advertised as routable,
    /// which means that our node can attempt outgoing connections to it
    pub fn new(ip: IpAddr, advertised: bool) -> PeerInfo {
        PeerInfo {
            ip,
            last_alive: None,
            last_failure: None,
            advertised,
            active_out_connection_attempts: 0,
            active_out_connections: 0,
            active_in_connections: 0,
            peer_type: Default::default(),
            banned: false,
        }
    }

    /// peer is ready to be retried, enough time has elapsed since last failure
    pub fn is_peer_ready(&self, wakeup_interval: MassaTime, now: MassaTime) -> bool {
        if let Some(last_failure) = self.last_failure {
            if let Some(last_alive) = self.last_alive {
                if last_alive > last_failure {
                    return true;
                }
            }
            return now
                .saturating_sub(last_failure)
                .saturating_sub(wakeup_interval)
                > MassaTime::from(0u64);
        }
        true
    }
}

/// Connection count for a category
#[derive(Default, Debug)]
pub struct ConnectionCount {
    /// Number of outgoing connections our node is currently trying to establish.
    /// We might be in the process of establishing a TCP connection or handshaking with the peer.
    pub active_out_connection_attempts: usize,
    /// Number of currently live (TCP connection active, handshake successful) outgoing connections
    pub active_out_connections: usize,
    /// Number of currently live (TCP connection active, handshake successful) incoming connections
    pub active_in_connections: usize,
}

impl ConnectionCount {
    #[inline]
    /// Gets available out connection attempts for given connection count and settings
    pub fn get_available_out_connection_attempts(&self, cfg: &PeerTypeConnectionConfig) -> usize {
        std::cmp::min(
            cfg.target_out_connections
                .saturating_sub(self.active_out_connection_attempts)
                .saturating_sub(self.active_out_connections),
            cfg.max_out_attempts
                .saturating_sub(self.active_out_connection_attempts),
        )
    }
}
