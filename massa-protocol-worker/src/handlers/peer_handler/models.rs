use crossbeam::channel::Sender;
use massa_protocol_exports::{BootstrapPeers, ProtocolError};
use parking_lot::RwLock;
use peernet::{peer_id::PeerId, transports::TransportType};
use rand::seq::SliceRandom;
use std::cmp::Reverse;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    collections::{BTreeMap, HashMap},
    net::SocketAddr,
    sync::Arc,
};
use tracing::log::info;

use super::announcement::Announcement;

const THREE_DAYS_MS: u128 = 3 * 24 * 60 * 60 * 1_000_000;

pub type InitialPeers = HashMap<PeerId, HashMap<SocketAddr, TransportType>>;

#[derive(Default)]
pub struct PeerDB {
    pub peers: HashMap<PeerId, PeerInfo>,
    /// last is the oldest value (only routable peers)
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
    Ban(Vec<PeerId>),
    Unban(Vec<PeerId>),
    GetBootstrapPeers { responder: Sender<BootstrapPeers> },
    Stop,
}

pub struct PeerManagementChannel {
    pub msg_sender: Sender<PeerMessageTuple>,
    pub command_sender: Sender<PeerManagementCmd>,
}

impl PeerDB {
    pub fn ban_peer(&mut self, peer_id: &PeerId) {
        println!("peers: {:?}", self.peers);
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.state = PeerState::Banned;
            info!("Banned peer: {:?}", peer_id);
        } else {
            info!("Tried to ban unknown peer: {:?}", peer_id);
        };
    }

    pub fn unban_peer(&mut self, peer_id: &PeerId) {
        if self.peers.contains_key(peer_id) {
            self.peers.remove(peer_id);
            info!("Unbanned peer: {:?}", peer_id);
        } else {
            info!("Tried to unban unknown peer: {:?}", peer_id);
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
    pub fn get_rand_peers_to_send(
        &self,
        nb_peers: usize,
    ) -> Vec<(PeerId, HashMap<SocketAddr, TransportType>)> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backward")
            .as_millis();

        let min_time = now - THREE_DAYS_MS;

        let mut keys = self.peers.keys().cloned().collect::<Vec<_>>();
        let mut rng = rand::thread_rng();
        keys.shuffle(&mut rng);

        let mut result = Vec::new();

        for key in keys {
            if result.len() >= nb_peers {
                break;
            }
            if let Some(peer) = self.peers.get(&key) {
                // skip old peers
                if peer.last_announce.timestamp < min_time {
                    continue;
                }
                let listeners: HashMap<SocketAddr, TransportType> = peer
                    .last_announce
                    .listeners
                    .clone()
                    .into_iter()
                    .filter(|(addr, _)| addr.ip().to_canonical().is_global())
                    .collect();
                if listeners.is_empty() {
                    continue;
                }
                result.push((key, listeners));
            }
        }

        result
    }

    pub fn get_banned_peer_count(&self) -> u64 {
        self.peers
            .values()
            .filter(|peer| peer.state == PeerState::Banned)
            .count() as u64
    }

    // Flush PeerDB to disk ?
    fn _flush(&self) -> Result<(), ProtocolError> {
        unimplemented!()
    }
}
