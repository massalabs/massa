use std::collections::hash_map::Entry;

use massa_models::{
    operation::{OperationId, OperationIds, OperationPrefixId},
    prehash::Map,
};

/// The structure store the previously checked operations.
/// Manage the relation between [OperationPrefixId] and [OperationId]
/// note: we could think about replace `Vec<OperationId>` with `Vec<OperationSuffixId>`
///       if the execution time CPU is equivalent
#[derive(Default)]
pub(crate) struct CheckedOperations(Map<OperationPrefixId, OperationIds>);

impl CheckedOperations {
    /// Insert in the adapter an operation `id`.
    ///
    /// If the set did not have this value present, `true` is returned.
    ///
    /// If the set did have this value present, `false` is returned.
    pub fn insert(&mut self, id: &OperationId) -> bool {
        let prefix = id.prefix();
        match self.0.entry(prefix) {
            Entry::Occupied(mut e) => e.get_mut().insert(*id),
            Entry::Vacant(e) => {
                let mut ids = OperationIds::default();
                ids.insert(*id);
                e.insert(ids);
                true
            }
        }
    }

    /// Get a set of [OperationIds] matching with the givec `prefix`.
    pub fn get(&self, prefix: &OperationPrefixId) -> Option<&OperationIds> {
        self.0.get(prefix)
    }

    /// Clear the content of the adapter.
    pub fn clear(&mut self) {
        self.0.clear()
    }

    /// Returns the number of prefix keys in the adapter.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline(always)]
    pub fn contains(&self, prefix: &OperationPrefixId) -> bool {
        self.0.contains_key(&prefix)
    }
}
