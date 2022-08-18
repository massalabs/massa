use massa_models::{
    operation::{OperationId, OperationPrefixId},
    prehash::{Map, Set},
};

/// The structure store the previously checked operations.
/// Manage the relation between [OperationPrefixId] and [OperationId]
/// note: we could think about replace `Vec<OperationId>` with `Vec<OperationSuffixId>`
///       if the execution time CPU is equivalent
#[derive(Default)]
pub(crate) struct CheckedOperations(Map<OperationPrefixId, OperationId>);

impl CheckedOperations {
    /// Insert in the adapter an operation `id`.
    ///
    /// If the set did not have this value present, `true` is returned.
    ///
    /// If the set did have this value present, `false` is returned.
    pub fn insert(&mut self, id: &OperationId) -> bool {
        let prefix = id.prefix();
        self.0.insert(prefix, *id).is_none()
    }

    /// Get a operation id matching with the givec `prefix` or None if there is none.
    pub fn get(&self, prefix: &OperationPrefixId) -> Option<&OperationId> {
        self.0.get(prefix)
    }

    /// Clear the content of the adapter.
    pub fn clear(&mut self) -> Set<OperationId> {
        self.0.drain().map(|(_, id)| id).collect()
    }

    /// Returns the number of prefix keys in the adapter.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline(always)]
    pub fn contains(&self, prefix: &OperationPrefixId) -> bool {
        self.0.contains_key(prefix)
    }
}
