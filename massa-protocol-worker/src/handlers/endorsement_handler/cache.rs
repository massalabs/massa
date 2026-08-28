use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use massa_models::{
    config::MAX_ENDORSEMENTS_PER_SLOT_INDEX, endorsement::EndorsementId, prehash::PreHashSet,
    slot::Slot,
};
use massa_protocol_exports::PeerId;
use parking_lot::RwLock;
use schnellru::{ByLength, LruMap};

/// Cache of endorsements
pub struct EndorsementCache {
    /// List of endorsements we checked recently
    pub checked_endorsements: LruMap<EndorsementId, ()>,
    /// Endorsements accepted for a given `(inclusion slot, index)` draw, used to bound how many
    /// conflicting variants a single drawn endorser can push through us
    pub endorsements_by_draw: LruMap<(Slot, u32), PreHashSet<EndorsementId>>,
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
            // one entry per draw holds up to `MAX_ENDORSEMENTS_PER_SLOT_INDEX` ids, so scaling the
            // capacity down keeps this index's memory in line with `checked_endorsements` while
            // covering the same horizon of endorsements
            endorsements_by_draw: LruMap::new(ByLength::new(std::cmp::max(
                max_known_endorsements / MAX_ENDORSEMENTS_PER_SLOT_INDEX as u32,
                1,
            ))),
            endorsements_known_by_peer: HashMap::new(),
            max_known_endorsements_by_peer,
        }
    }

    /// Mark a list of endorsement IDs prefixes as known by a peer
    pub fn insert_peer_known_endorsements(
        &mut self,
        peer_id: &PeerId,
        endorsements: &[EndorsementId],
    ) {
        let known_endorsements = self
            .endorsements_known_by_peer
            .entry(*peer_id)
            .or_insert_with(|| LruMap::new(ByLength::new(self.max_known_endorsements_by_peer)));
        for endorsement in endorsements {
            known_endorsements.insert(*endorsement, ());
        }
    }

    /// Mark an endorsement ID as checked by us
    pub fn insert_checked_endorsement(&mut self, enrodsement_id: EndorsementId) {
        self.checked_endorsements.insert(enrodsement_id, ());
    }

    /// Register an endorsement against its `(slot, index)` draw and tell whether we accept it.
    ///
    /// The endorsement id is derived from the whole signed content, so an equivocating endorser can
    /// mint unlimited distinct-but-valid endorsements for the draw it won just by varying the
    /// endorsed block: deduplicating on `checked_endorsements` alone does not stop them from being
    /// noted and gossiped further. Returns `false` once `MAX_ENDORSEMENTS_PER_SLOT_INDEX` distinct
    /// endorsements have already been registered for that draw.
    ///
    /// Registering the id rather than only counting keeps this idempotent, so an endorsement that
    /// gets re-checked after being evicted from `checked_endorsements` does not consume the budget
    /// twice.
    pub fn register_draw_endorsement(
        &mut self,
        slot: Slot,
        index: u32,
        endorsement_id: EndorsementId,
    ) -> bool {
        let Some(draw_endorsements) = self
            .endorsements_by_draw
            .get_or_insert((slot, index), PreHashSet::default)
        else {
            return false;
        };
        if draw_endorsements.contains(&endorsement_id) {
            return true;
        }
        if draw_endorsements.len() >= MAX_ENDORSEMENTS_PER_SLOT_INDEX {
            return false;
        }
        draw_endorsements.insert(endorsement_id);
        true
    }

    /// Update caches to remove all data from disconnected peers
    pub fn update_cache(&mut self, peers_connected: &HashSet<PeerId>) {
        // Remove disconnected peers from cache
        self.endorsements_known_by_peer
            .retain(|peer_id, _| peers_connected.contains(peer_id));

        // Add new connected peers to cache
        for peer_id in peers_connected {
            match self.endorsements_known_by_peer.entry(*peer_id) {
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
