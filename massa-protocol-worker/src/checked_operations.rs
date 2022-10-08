// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Cache of previously successfully checked operations and their prefix IDs

use massa_models::operation::{OperationId, OperationPrefixId};

use crate::cache::LinearHashCacheSet;

/// Checked operations cache.
/// Note: prefix and ID caches are expected to get durably desynchronized in case of prefix collision.
#[derive(Debug, Clone)]
pub struct CheckedOperations {
    /// Linear cache of operation IDs
    op_ids: LinearHashCacheSet<OperationId>,
    /// Linear cache and counter of operation ID prefixes
    op_prefixes: LinearHashCacheSet<OperationPrefixId>,
}

impl CheckedOperations {
    /// Create a new checked operations cache
    pub fn new(capacity: usize) -> Self {
        CheckedOperations {
            op_ids: LinearHashCacheSet::new(capacity),
            op_prefixes: LinearHashCacheSet::new(capacity),
        }
    }

    /// Insert an operation ID and its deduced prefix
    pub fn insert(&mut self, operation_id: OperationId) {
        self.op_ids.try_insert(operation_id);
        self.op_prefixes.try_insert(operation_id.prefix());
    }

    /// Check if the cache contains a given operation ID
    pub fn contains_id(&self, operation_id: &OperationId) -> bool {
        self.op_ids.contains(operation_id)
    }

    /// Check if the cache contains a given operation ID prefix
    pub fn contains_prefix(&self, prefix: &OperationPrefixId) -> bool {
        self.op_prefixes.contains(prefix)
    }

    /// Extend with new IDs
    pub fn extend<I: IntoIterator<Item = OperationId>>(&mut self, iter: I) {
        iter.into_iter().for_each(|id| {
            self.insert(id);
        });
    }
}
