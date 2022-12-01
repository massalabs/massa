use crate::settings::PeerTypeConnectionConfig;
use displaydoc::Display;
use enum_map::Enum;
use massa_models::node::NodeId;
use massa_models::serialization::{IpAddrDeserializer, IpAddrSerializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};
use massa_time::MassaTime;
use nom::error::{ContextError, ParseError};
use nom::multi::length_count;
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::ops::Bound::Included;
use std::{collections::HashMap, net::IpAddr};
/// Associate a peer info with nodes
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Peer {
    /// peer info
    pub peer_info: PeerInfo,
    /// corresponding nodes (true if the connection is outgoing, false if incoming)
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapPeers(pub Vec<IpAddr>);

/// Serializer for `BootstrapPeers`
pub struct BootstrapPeersSerializer {
    u32_serializer: U32VarIntSerializer,
    ip_addr_serializer: IpAddrSerializer,
}

impl BootstrapPeersSerializer {
    /// Creates a new `BootstrapPeersSerializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            ip_addr_serializer: IpAddrSerializer::new(),
        }
    }
}

impl Default for BootstrapPeersSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<BootstrapPeers> for BootstrapPeersSerializer {
    /// ```
    /// use massa_network_exports::{BootstrapPeers, BootstrapPeersSerializer};
    /// use massa_serialization::Serializer;
    /// use std::str::FromStr;
    /// use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    ///
    /// let localhost_v4 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    /// let localhost_v6 = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
    /// let mut serialized = Vec::new();
    /// let peers = BootstrapPeers(vec![localhost_v4, localhost_v6]);
    /// let peers_serializer = BootstrapPeersSerializer::new();
    /// peers_serializer.serialize(&peers, &mut serialized).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &BootstrapPeers,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        let peers_count: u32 = value.0.len().try_into().map_err(|err| {
            SerializeError::NumberTooBig(format!(
                "too many peers blocks in BootstrapPeers: {}",
                err
            ))
        })?;
        self.u32_serializer.serialize(&peers_count, buffer)?;
        for peer in value.0.iter() {
            self.ip_addr_serializer.serialize(peer, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `BootstrapPeers`
pub struct BootstrapPeersDeserializer {
    length_deserializer: U32VarIntDeserializer,
    ip_addr_deserializer: IpAddrDeserializer,
}

impl BootstrapPeersDeserializer {
    /// Creates a new `BootstrapPeersDeserializer`
    ///
    /// Arguments:
    ///
    /// * `max_peers`: maximum peers that can be serialized
    pub fn new(max_peers: u32) -> Self {
        Self {
            length_deserializer: U32VarIntDeserializer::new(Included(0), Included(max_peers)),
            ip_addr_deserializer: IpAddrDeserializer::new(),
        }
    }
}

impl Deserializer<BootstrapPeers> for BootstrapPeersDeserializer {
    /// ```
    /// use massa_network_exports::{BootstrapPeers, BootstrapPeersSerializer, BootstrapPeersDeserializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use std::str::FromStr;
    /// use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    ///
    /// let localhost_v4 = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    /// let localhost_v6 = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
    /// let mut serialized = Vec::new();
    /// let peers = BootstrapPeers(vec![localhost_v4, localhost_v6]);
    /// let peers_serializer = BootstrapPeersSerializer::new();
    /// let peers_deserializer = BootstrapPeersDeserializer::new(1000);
    /// peers_serializer.serialize(&peers, &mut serialized).unwrap();
    /// let (rest, peers_deser) = peers_deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(peers, peers_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BootstrapPeers, E> {
        length_count(
            |input| self.length_deserializer.deserialize(input),
            |input| self.ip_addr_deserializer.deserialize(input),
        )
        .map(BootstrapPeers)
        .parse(buffer)
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
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord,Serialize, Deserialize, Debug)]
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
        self.banned = false;
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
                > MassaTime::from_millis(0u64);
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
