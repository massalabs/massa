use massa_models::{prehash::Map, Address, BlockId, OperationId, OperationIds, WrappedOperation};

/// Container for all operations and different indexes.
/// Note: The structure can evolve and store more indexes.
#[derive(Default)]
pub struct OperationIndexes {
    /// Operation structure container
    pub(crate) operations: Map<OperationId, WrappedOperation>,
    /// Structure mapping creators with the created operations
    index_by_creator: Map<Address, OperationIds>,
    /// Structure mapping block ids with the operations
    index_by_block: Map<BlockId, OperationIds>,
}

impl OperationIndexes {
    pub(crate) fn batch_insert(&mut self, operations: Vec<WrappedOperation>) {
        for operation in operations {
            let id = operation.id;
            let creator = operation.creator_address;
            self.operations.entry(id).or_insert(operation);
            self.index_by_creator.entry(creator).or_default().insert(id);
        }
    }

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

    pub(crate) fn link_operations_with_block(
        &mut self,
        block_id: &BlockId,
        operations: &OperationIds,
    ) {
        self.index_by_block
            .entry(*block_id)
            .or_default()
            .extend(operations);
    }

    pub(crate) fn unlink_operations_from_block(&mut self, block_id: &BlockId) {
        self.index_by_block.remove(block_id);
    }

    pub fn get_operations_created_by(&self, address: &Address) -> Vec<OperationId> {
        match self.index_by_creator.get(address) {
            Some(operations) => operations.iter().cloned().collect(),
            None => Vec::new(),
        }
    }

    pub fn get_operations_in_block(&self, block_id: &BlockId) -> Vec<OperationId> {
        match self.index_by_block.get(block_id) {
            Some(ids) => ids.iter().cloned().collect(),
            None => Vec::new(),
        }
    }
}
