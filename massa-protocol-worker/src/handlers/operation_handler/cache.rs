use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use massa_models::operation::{OperationId, OperationPrefixId};
use massa_protocol_exports::PeerId;
use parking_lot::RwLock;
use schnellru::{ByLength, LruMap};

pub struct OperationCache {
    pub checked_operations: LruMap<OperationId, ()>,
    pub checked_operations_prefix: LruMap<OperationPrefixId, ()>,
    pub ops_known_by_peer: HashMap<PeerId, LruMap<OperationPrefixId, ()>>,
    pub max_known_ops_by_peer: u32,
}

impl OperationCache {
    pub fn new(max_known_ops: u32, max_known_ops_by_peer: u32) -> Self {
        Self {
            checked_operations: LruMap::new(ByLength::new(max_known_ops)),
            checked_operations_prefix: LruMap::new(ByLength::new(max_known_ops)),
            ops_known_by_peer: HashMap::new(),
            max_known_ops_by_peer,
        }
    }

    pub fn insert_checked_operation(&mut self, operation_id: OperationId) {
        self.checked_operations.insert(operation_id, ());
        self.checked_operations_prefix
            .insert(operation_id.prefix(), ());
    }

    pub fn update_cache(&mut self, peers_connected: &HashSet<PeerId>) {
        // Remove disconnected peers from cache
        self.ops_known_by_peer
            .retain(|peer_id, _| !peers_connected.contains(peer_id));

        // Add new connected peers to cache
        for peer_id in peers_connected {
            match self.ops_known_by_peer.entry(peer_id.clone()) {
                std::collections::hash_map::Entry::Occupied(_) => {}
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(LruMap::new(ByLength::new(self.max_known_ops_by_peer)));
                }
            }
        }
    }
}

pub type SharedOperationCache = Arc<RwLock<OperationCache>>;
