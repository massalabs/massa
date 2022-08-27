use std::collections::hash_map;

use massa_models::{
    prehash::{PreHashMap, PreHashSet},
    Address, OperationId, WrappedOperation,
};

/// Container for all operations and different indexes.
/// Note: The structure can evolve and store more indexes.
#[derive(Default)]
pub struct OperationIndexes {
    /// Operations structure container
    operations: PreHashMap<OperationId, WrappedOperation>,
    /// Structure mapping creators with the created operations
    index_by_creator: PreHashMap<Address, PreHashSet<OperationId>>,
}

impl OperationIndexes {
    /// Insert an operation and populate the indexes.
    /// Arguments:
    /// - operation: the operation to insert
    pub(crate) fn insert(&mut self, operation: WrappedOperation) {
        if let Ok(o) = self.operations.try_insert(operation.id, operation) {
            // update creator index
            self.index_by_creator
                .entry(o.creator_address)
                .or_default()
                .insert(o.id);
        }
    }

    /// Remove a operation, remove from the indexes and made some clean-up in indexes if necessary.
    /// Arguments:
    /// - operation_id: the operation id to remove
    pub(crate) fn remove(&mut self, operation_id: &OperationId) -> Option<WrappedOperation> {
        if let Some(o) = self.operations.remove(operation_id) {
            // update creator index
            if let hash_map::Entry::Occupied(mut occ) =
                self.index_by_creator.entry(o.creator_address)
            {
                occ.get_mut().remove(&o.id);
                if occ.get().is_empty() {
                    occ.remove();
                }
            }
            return Some(o);
        }
        None
    }

    /// Gets a reference to a stored operation, if any.
    pub fn get(&self, id: &OperationId) -> Option<&WrappedOperation> {
        self.operations.get(id)
    }

    /// Checks whether an operation exists in global storage.
    pub fn contains(&self, id: &OperationId) -> bool {
        self.operations.contains_key(id)
    }

    /// Get operations created by an address
    /// Arguments:
    /// - address: the address to get the operations created by
    ///
    /// Returns:
    /// - optional reference to a set of operations created by that address
    pub fn get_operations_created_by(&self, address: &Address) -> Option<&PreHashSet<OperationId>> {
        self.index_by_creator.get(address)
    }
}
