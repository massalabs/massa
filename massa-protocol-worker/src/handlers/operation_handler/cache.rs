use std::{num::NonZeroUsize, sync::Arc};

use lru::LruCache;
use massa_models::operation::{OperationId, OperationPrefixId};
use parking_lot::RwLock;
use peernet::peer_id::PeerId;

pub(crate)  struct OperationCache {
    pub(crate)  checked_operations: LruCache<OperationId, ()>,
    pub(crate)  checked_operations_prefix: LruCache<OperationPrefixId, ()>,
    pub(crate)  ops_known_by_peer: LruCache<PeerId, LruCache<OperationPrefixId, ()>>,
}

impl OperationCache {
    pub(crate)  fn new(max_known_ops: NonZeroUsize, max_known_ops_by_peer: NonZeroUsize) -> Self {
        Self {
            checked_operations: LruCache::new(max_known_ops),
            checked_operations_prefix: LruCache::new(max_known_ops),
            ops_known_by_peer: LruCache::new(max_known_ops_by_peer),
        }
    }

    pub(crate)  fn insert_checked_operation(&mut self, operation_id: OperationId) {
        self.checked_operations.put(operation_id, ());
        self.checked_operations_prefix
            .put(operation_id.prefix(), ());
    }
}

pub(crate)  type SharedOperationCache = Arc<RwLock<OperationCache>>;
