use std::sync::Arc;

use massa_models::operation::{OperationId, OperationPrefixId};
use massa_protocol_exports::PeerId;
use parking_lot::RwLock;
use schnellru::{ByLength, LruMap};

pub struct OperationCache {
    pub checked_operations: LruMap<OperationId, ()>,
    pub checked_operations_prefix: LruMap<OperationPrefixId, ()>,
    pub ops_known_by_peer: LruMap<PeerId, LruMap<OperationPrefixId, ()>>,
}

impl OperationCache {
    pub fn new(max_known_ops: u32, max_known_ops_by_peer: u32) -> Self {
        Self {
            checked_operations: LruMap::new(ByLength::new(max_known_ops)),
            checked_operations_prefix: LruMap::new(ByLength::new(max_known_ops)),
            ops_known_by_peer: LruMap::new(ByLength::new(max_known_ops_by_peer)),
        }
    }

    pub fn insert_checked_operation(&mut self, operation_id: OperationId) {
        self.checked_operations.insert(operation_id, ());
        self.checked_operations_prefix
            .insert(operation_id.prefix(), ());
    }
}

pub type SharedOperationCache = Arc<RwLock<OperationCache>>;
