use crossbeam::channel::Sender;
use massa_protocol_exports_2::ProtocolError;
use parking_lot::RwLock;
use peernet::{peer_id::PeerId, transports::TransportType};
use std::cmp::Reverse;
use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    sync::Arc,
};
use tracing::log::info;

use super::announcement::Announcement;

pub type InitialPeers = HashMap<PeerId, HashMap<SocketAddr, TransportType>>;

#[derive(Default)]
pub struct PeerDB {
    pub peers: HashMap<PeerId, PeerInfo>,
    /// last is the oldest value
    pub index_by_newest: BTreeMap<Reverse<u128>, PeerId>,
}

pub type SharedPeerDB = Arc<RwLock<PeerDB>>;

pub type PeerMessageTuple = (PeerId, u64, Vec<u8>);

#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub last_announce: Announcement,
    pub state: PeerState,
}

#[warn(dead_code)]
#[derive(Eq, PartialEq, Clone, Debug)]
pub enum PeerState {
    Banned,
    InHandshake,
    HandshakeFailed,
    Trusted,
}

pub enum PeerManagementCmd {
    Ban(PeerId),
    Stop,
}

pub struct PeerManagementChannel {
    pub msg_sender: Sender<PeerMessageTuple>,
    pub command_sender: Sender<PeerManagementCmd>,
}

impl PeerDB {
    pub fn ban_peer(&mut self, peer_id: &PeerId) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.state = PeerState::Banned;
            info!("Banned peer: {:?}", peer_id);
        } else {
            info!("Tried to ban unknown peer: {:?}", peer_id);
        };
    }

    /// get best peers for a given number of peers
    /// returns a vector of peer ids
    pub fn get_best_peers(&self, nb_peers: usize) -> Vec<PeerId> {
        self.index_by_newest
            .iter()
            .filter_map(|(_, peer_id)| {
                self.peers.get(peer_id).and_then(|peer| {
                    if peer.state == PeerState::Trusted {
                        Some(peer_id.clone())
                    } else {
                        None
                    }
                })
            })
            .take(nb_peers)
            .collect()
    }

    /// Retrieve the peer with the oldest test date.
    pub fn get_oldest_peer(&self) -> Option<(PeerId, PeerInfo)> {
        self.index_by_newest.last_key_value().map(|data| {
            let peer_id = data.1.clone();
            let peer_info = self
                .peers
                .get(&peer_id)
                .unwrap_or_else(|| panic!("Peer {:?} not found", peer_id))
                .clone();
            (peer_id, peer_info)
        })
    }

    // Flush PeerDB to disk ?
    fn flush(&self) -> Result<(), ProtocolError> {
        unimplemented!()
    }
}
