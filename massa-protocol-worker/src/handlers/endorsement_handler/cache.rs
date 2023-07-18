use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use massa_models::endorsement::EndorsementId;
use massa_protocol_exports::PeerId;
use parking_lot::RwLock;
use schnellru::{ByLength, LruMap};

/// Cache of endorsements
pub struct EndorsementCache {
    /// List of endorsements we checked recently
    pub checked_endorsements: LruMap<EndorsementId, ()>,
    /// List of endorsements known by peers
    pub endorsements_known_by_peer: HashMap<PeerId, LruMap<EndorsementId, ()>>,
    /// Maximum number of endorsements known by a peer
    pub max_known_endorsements_by_peer: u32,
}

impl EndorsementCache {
    /// Create a new EndorsementCache
    pub fn new(max_known_endorsements: u32, max_known_endorsements_by_peer: u32) -> Self {
        Self {
            checked_endorsements: LruMap::new(ByLength::new(max_known_endorsements)),
            endorsements_known_by_peer: HashMap::new(),
            max_known_endorsements_by_peer,
        }
    }

    /// Mark a list of endorsement IDs prefixes as known by a peer
    pub fn insert_peer_known_endorsements(
        &mut self,
        peer_id: PeerId,
        endorsements: &[EndorsementId],
    ) {
        let known_endorsements = self
            .endorsements_known_by_peer
            .entry(peer_id)
            .or_insert_with(|| LruMap::new(ByLength::new(self.max_known_endorsements_by_peer)));
        for endorsement in endorsements {
            known_endorsements.insert(*endorsement, ());
        }
    }

    /// Mark an endorsement ID as checked by us
    pub fn insert_checked_endorsement(&mut self, enrodsement_id: EndorsementId) {
        self.checked_endorsements.insert(enrodsement_id, ());
    }

    /// Update caches to remove all data from disconnected peers
    pub fn update_cache(&mut self, peers_connected: &HashSet<PeerId>) {
        // Remove disconnected peers from cache
        self.endorsements_known_by_peer
            .retain(|peer_id, _| !peers_connected.contains(peer_id));

        // Add new connected peers to cache
        for peer_id in peers_connected {
            match self.endorsements_known_by_peer.entry(peer_id.clone()) {
                std::collections::hash_map::Entry::Occupied(_) => {}
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(LruMap::new(ByLength::new(
                        self.max_known_endorsements_by_peer,
                    )));
                }
            }
        }
    }
}

pub type SharedEndorsementCache = Arc<RwLock<EndorsementCache>>;
