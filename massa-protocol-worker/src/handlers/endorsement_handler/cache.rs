use std::sync::Arc;

use massa_models::endorsement::EndorsementId;
use massa_protocol_exports::PeerId;
use parking_lot::RwLock;
use schnellru::{ByLength, LruMap};

pub struct EndorsementCache {
    pub checked_endorsements: LruMap<EndorsementId, ()>,
    pub endorsements_known_by_peer: LruMap<PeerId, LruMap<EndorsementId, ()>>,
}

impl EndorsementCache {
    pub fn new(max_known_endorsements: u32, max_known_endorsements_by_peer: u32) -> Self {
        Self {
            checked_endorsements: LruMap::new(ByLength::new(max_known_endorsements)),
            endorsements_known_by_peer: LruMap::new(ByLength::new(max_known_endorsements_by_peer)),
        }
    }

    pub fn update_cache(
        &mut self,
        peers_connected: HashSet<PeerId>,
        max_known_endorsements_by_peer: u32,
    ) {
        let peers: Vec<PeerId> = self
            .endorsements_known_by_peer
            .iter()
            .map(|(id, _)| id.clone())
            .collect();

        // Clean shared cache if peers do not exist anymore
        for peer_id in peers {
            if !peers_connected.contains(&peer_id) {
                self.endorsements_known_by_peer.remove(&peer_id);
            }
        }

        // Add new potential peers
        for peer_id in peers_connected {
            if self.endorsements_known_by_peer.peek(&peer_id).is_none() {
                self.endorsements_known_by_peer.insert(
                    peer_id.clone(),
                    LruMap::new(ByLength::new(max_known_endorsements_by_peer)),
                );
            }
        }
    }
}

pub type SharedEndorsementCache = Arc<RwLock<EndorsementCache>>;
