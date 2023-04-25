use crossbeam::channel::Sender;
use massa_protocol_exports_2::ProtocolError;
use parking_lot::RwLock;
use peernet::{peer_id::PeerId, transports::TransportType};
use std::cmp::Reverse;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    sync::Arc,
};
use tracing::log::info;

use super::announcement::Announcement;

const THREE_DAYS_NS: u128 = 3 * 24 * 60 * 60 * 1_000_000_000;

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

    /// Select max 100 peers to send to another peer
    /// The selected peers should has been online within the last 3 days
    pub fn get_peers_to_send(&self) -> Vec<(PeerId, HashMap<SocketAddr, TransportType>)> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backward")
            .as_nanos();
        let min_time = now - THREE_DAYS_NS;
        //  todo update for random
        self.peers
            .iter()
            .filter_map(|(k, v)| {
                if v.last_announce.timestamp < min_time {
                    None
                } else {
                    Some((k.clone(), v.last_announce.listeners.clone()))
                }
            })
            .take(100)
            .collect()

        // self.index_by_newest
        //     .iter()
        //     .filter_map(|(timestamp, peer_id)| {
        //         if timestamp.0 < min_time {
        //             None
        //         } else {
        //             self.peers.get(peer_id).and_then(|peer| {
        //                 if peer.last_announce.listeners.is_empty() {
        //                     None
        //                 } else {
        //                     Option::from((peer_id.clone(), peer.last_announce.listeners.clone()))
        //                 }
        //             })
        //         }
        //     })
        //     .take(100)
        //     .collect()
    }

    // Flush PeerDB to disk ?
    fn flush(&self) -> Result<(), ProtocolError> {
        unimplemented!()
    }
}
