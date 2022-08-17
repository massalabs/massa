use massa_models::{
    prehash::{Map, Set},
    Address, BlockId, OperationId, WrappedOperation,
};

/// Container for all operations and different indexes.
/// Note: The structure can evolve and store more indexes.
#[derive(Default)]
pub struct OperationIndexes {
    /// Operation structure container
    pub(crate) operations: Map<OperationId, WrappedOperation>,
    /// Structure mapping creators with the created operations
    index_by_creator: Map<Address, Set<OperationId>>,
    /// Structure mapping block ids with the operations
    index_by_block: Map<BlockId, Vec<OperationId>>,
}

impl OperationIndexes {
    /// Insert a batch of operations and populate the indexes.
    /// Arguments:
    /// - operations: the operations to insert
    pub(crate) fn batch_insert(&mut self, operations: Vec<WrappedOperation>) {
        for operation in operations {
            let id = operation.id;
            let creator = operation.creator_address;
            self.operations.entry(id).or_insert(operation);
            self.index_by_creator.entry(creator).or_default().insert(id);
        }
    }

    /// Remove a batch of operations, remove from the indexes and made some clean-up in indexes if necessary.
    /// Arguments:
    /// - operation_ids: the operation ids to remove
    pub(crate) fn batch_remove(&mut self, operation_ids: Vec<OperationId>) {
        for id in operation_ids {
            let operation = self
                .operations
                .remove(&id)
                .expect("removing absent object from storage");
            let creator = operation.creator_address;
            let entry = self.index_by_creator.entry(creator).or_default();
            entry.remove(&id);
            if entry.is_empty() {
                self.index_by_creator.remove(&creator);
            }
        }
    }

    /// Link a vector of operations to a block. Should be used in case of the block is added to the storage
    /// Arguments:
    /// - block_id: the block id to link the operations to
    /// - operation_ids: the operations to link to the block
    pub(crate) fn link_operations_with_block(
        &mut self,
        block_id: &BlockId,
        operations: &Vec<OperationId>,
    ) {
        self.index_by_block
            .entry(*block_id)
            .or_default()
            .extend(operations);
    }

    /// Unlink a vector of operations from a block. Should be used in case of the block is removed from the storage.
    /// Arguments:
    /// - block_id: the block id to unlink the operations from
    pub(crate) fn unlink_operations_from_block(&mut self, block_id: &BlockId) {
        self.index_by_block.remove(block_id);
    }

    /// Get the operations created by an address.
    /// Arguments:
    /// - address: the address of the creator
    ///
    /// Returns:
    /// - the operations created by the address
    pub fn get_operations_created_by(&self, address: &Address) -> Vec<OperationId> {
        match self.index_by_creator.get(address) {
            Some(operations) => operations.iter().cloned().collect(),
            None => Vec::new(),
        }
    }

    /// Get the operations linked to a block.
    /// Arguments:
    /// - block_id: the block id to get the operations from
    ///
    /// Returns:
    /// - the operations linked to the block
    pub fn get_operations_in_block(&self, block_id: &BlockId) -> Vec<OperationId> {
        match self.index_by_block.get(block_id) {
            Some(ids) => ids.iter().cloned().collect(),
            None => Vec::new(),
        }
    }
}
