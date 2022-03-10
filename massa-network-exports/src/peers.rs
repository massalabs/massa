use std::{collections::HashMap, net::IpAddr};

use massa_models::{
    node::NodeId, with_serialization_context, DeserializeCompact, DeserializeVarInt, ModelsError,
    SerializeCompact, SerializeVarInt,
};
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub peer_info: PeerInfo,
    pub active_nodes: Vec<(NodeId, bool)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peers {
    pub our_node_id: NodeId,
    pub peers: HashMap<IpAddr, Peer>,
}

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

/// All information concerning a peer is here
#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct PeerInfo {
    /// Peer ip address.
    pub ip: IpAddr,
    /// If peer is banned.
    pub banned: bool,
    /// If peer is boostrap, ie peer was in initial peer file
    pub bootstrap: bool,
    /// Time in millis when peer was last alive
    pub last_alive: Option<MassaTime>,
    /// Time in millis of peer's last failure
    pub last_failure: Option<MassaTime>,
    /// Whether peer was promoted through another peer
    pub advertised: bool,

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
    /// Returns true if there is at least one connection attempt /
    ///  one active connection in either direction
    /// with this peer
    pub fn is_active(&self) -> bool {
        self.active_out_connection_attempts > 0
            || self.active_out_connections > 0
            || self.active_in_connections > 0
    }
}
