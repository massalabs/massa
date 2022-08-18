use std::collections::hash_map;

use massa_models::{
    prehash::{Map, Set},
    Address, BlockId, OperationId, WrappedOperation,
};

/// Container for all operations and different indexes.
/// Note: The structure can evolve and store more indexes.
#[derive(Default)]
pub struct OperationIndexes {
    /// Operations structure container
    operations: Map<OperationId, WrappedOperation>,
    /// Structure mapping creators with the created operations
    index_by_creator: Map<Address, Set<OperationId>>,
    /// Structure mapping operation IDs to their containing blocks
    index_to_block: Map<OperationId, Set<BlockId>>,
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

    /// Link a vector of operations to a block. Should be used in case of the block is added to the storage.
    /// Arguments:
    /// - block_id: the block id to link the operations to
    /// - operation_ids: the operations to link to the block
    pub(crate) fn link_block(&mut self, block_id: &BlockId, operation_ids: &Vec<OperationId>) {
        for operation_id in operation_ids {
            // update to-block index
            self.index_to_block
                .entry(*operation_id)
                .or_default()
                .insert(*block_id);
        }
    }

    /// Unlinks a block from the operations. Should be used when a block is removed from storage.
    /// Arguments:
    /// - block_id: the block id to unlink the operations from
    /// - operation_ids: the operations to unlink from the block
    pub(crate) fn unlink_block(&mut self, block_id: &BlockId, operation_ids: &Vec<OperationId>) {
        for operation_id in operation_ids {
            if let hash_map::Entry::Occupied(mut occ) = self.index_to_block.entry(*operation_id) {
                occ.get_mut().remove(block_id);
                if occ.get().is_empty() {
                    occ.remove();
                }
            }
        }
    }

    /// Remove a operation, remove from the indexes and made some clean-up in indexes if necessary.
    /// Arguments:
    /// - operation_id: the operation id to remove
    pub(crate) fn remove(&mut self, operation_id: &OperationId) -> Option<WrappedOperation> {
        // update to-block index
        self.index_to_block.remove(operation_id);

        if let Some(o) = self.operations.remove(operation_id) {
            // update creator index
            self.index_by_creator
                .entry(o.creator_address)
                .and_modify(|s| {
                    s.remove(&o.id);
                });

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
    pub fn get_operations_created_by(&self, address: &Address) -> Option<&Set<OperationId>> {
        self.index_by_creator.get(address)
    }

    /// Get the blocks that contain a given operation
    /// Arguments:
    /// - operation_id: the id of the operation
    ///
    /// Returns:
    /// - an optional reference to a set of block IDs containing the operation
    pub fn get_containing_blocks(&self, operation_id: &OperationId) -> Option<&Set<BlockId>> {
        self.index_to_block.get(operation_id)
    }
}
