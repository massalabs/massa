use std::{num::NonZeroUsize, sync::Arc};

use lru::LruCache;
use massa_models::endorsement::EndorsementId;
use parking_lot::RwLock;
use peernet::peer_id::PeerId;

pub struct EndorsementCache {
    pub checked_endorsements: LruCache<EndorsementId, ()>,
    pub endorsements_known_by_peer: LruCache<PeerId, LruCache<EndorsementId, ()>>,
}

impl EndorsementCache {
    pub fn new(
        max_known_endorsements: NonZeroUsize,
        max_known_endorsements_by_peer: NonZeroUsize,
    ) -> Self {
        Self {
            checked_endorsements: LruCache::new(max_known_endorsements),
            endorsements_known_by_peer: LruCache::new(max_known_endorsements_by_peer),
        }
    }

    pub fn insert_checked_endorsement(&mut self, endorsement_id: EndorsementId) {
        self.checked_endorsements.put(endorsement_id, ());
    }
}

pub type SharedEndorsementCache = Arc<RwLock<EndorsementCache>>;
