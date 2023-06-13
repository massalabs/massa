use std::{collections::HashSet, sync::Arc};

use massa_models::operation::{OperationId, OperationPrefixId};
use massa_protocol_exports::PeerId;
use parking_lot::RwLock;
use schnellru::{ByLength, LruMap};

pub struct OperationCache {
    pub checked_operations: LruMap<OperationId, ()>,
    pub checked_operations_prefix: LruMap<OperationPrefixId, ()>,
    pub ops_known_by_peer: LruMap<PeerId, LruMap<OperationPrefixId, ()>>,
    pub max_known_ops_by_peer: u32,
}

impl OperationCache {
    pub fn new(max_known_ops: u32, max_known_ops_by_peer: u32, max_peers: u32) -> Self {
        Self {
            checked_operations: LruMap::new(ByLength::new(max_known_ops)),
            checked_operations_prefix: LruMap::new(ByLength::new(max_known_ops)),
            ops_known_by_peer: LruMap::new(ByLength::new(max_peers)),
            max_known_ops_by_peer,
        }
    }

    pub fn insert_checked_operation(&mut self, operation_id: OperationId) {
        self.checked_operations.insert(operation_id, ());
        self.checked_operations_prefix
            .insert(operation_id.prefix(), ());
    }

    pub fn update_cache(&mut self, peers_connected: HashSet<PeerId>, _max_known_ops_by_peer: u32) {
        let peers: Vec<PeerId> = self
            .ops_known_by_peer
            .iter()
            .map(|(id, _)| id.clone())
            .collect();

        // Clean shared cache if peers do not exist anymore
        for peer_id in peers {
            if !peers_connected.contains(&peer_id) {
                self.ops_known_by_peer.remove(&peer_id);
            }
        }

        // Add new potential peers
        for peer_id in peers_connected {
            if self.ops_known_by_peer.peek(&peer_id).is_none() {
                self.ops_known_by_peer.insert(
                    peer_id.clone(),
                    LruMap::new(ByLength::new(self.max_known_ops_by_peer)),
                );
            }
        }
    }
}

pub type SharedOperationCache = Arc<RwLock<OperationCache>>;
