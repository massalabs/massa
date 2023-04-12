use crossbeam::channel::Sender;
use massa_protocol_exports_2::ProtocolError;
use parking_lot::RwLock;
use peernet::{peer_id::PeerId, transports::TransportType};
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
    pub index_by_newest: BTreeMap<u128, PeerId>,
}

pub type SharedPeerDB = Arc<RwLock<PeerDB>>;

pub type PeerMessageTuple = (PeerId, u64, Vec<u8>);

pub struct PeerInfo {
    pub last_announce: Announcement,
    pub state: PeerState,
}

#[warn(dead_code)]
#[derive(Eq, PartialEq)]
pub enum PeerState {
    Connected,
    Banned,
    InHandshake,
    HandshakeFailed,
    HandshakeSuccess,
    Trusted,
}

#[warn(dead_code)]
pub enum PeerManagementCmd {
    Ban(PeerId),
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

    // Flush PeerDB to disk ?
    fn flush(&self) -> Result<(), ProtocolError> {
        unimplemented!()
    }
}
