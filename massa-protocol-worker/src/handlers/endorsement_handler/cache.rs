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
}

pub type SharedEndorsementCache = Arc<RwLock<EndorsementCache>>;
