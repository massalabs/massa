//! This crate is used to share blocks across the node

#![warn(missing_docs)]

use massa_logging::massa_trace;
use massa_models::prehash::{Map, Set};
use massa_models::{BlockId, OperationId, WrappedBlock, WrappedOperation};
use parking_lot::RwLock;
use std::collections::hash_map::Entry;
use std::sync::Arc;

/// A storage of block, shared by various components.
#[derive(Clone, Default)]
pub struct Storage {
    blocks: Arc<RwLock<Map<BlockId, Arc<RwLock<WrappedBlock>>>>>,
    operations: Arc<RwLock<Map<OperationId, WrappedOperation>>>,
}

impl Storage {
    /// Store a block, along with it's serialized representation.
    pub fn store_block(&self, block: WrappedBlock) {
        massa_trace!("storage.storage.store_block", { "block_id": block.id });
        let mut blocks = self.blocks.write();
        match blocks.entry(block.id) {
            Entry::Occupied(_) => {}
            Entry::Vacant(entry) => {
                let to_store = Arc::new(RwLock::new(block));
                entry.insert(to_store);
            }
        }
    }

    /// Get a (mutable) reference to the stored block.
    pub fn retrieve_block(&self, block_id: &BlockId) -> Option<Arc<RwLock<WrappedBlock>>> {
        massa_trace!("storage.storage.retrieve_block", { "block_id": block_id });
        let blocks = self.blocks.read();
        blocks.get(block_id).map(Arc::clone)
    }

    /// Remove a list of blocks from storage.
    pub fn remove_blocks(&self, block_ids: &[BlockId]) {
        massa_trace!("storage.storage.remove_blocks", { "block_ids": block_ids });
        let mut blocks = self.blocks.write();
        for id in block_ids {
            blocks.remove(id);
        }
    }

    /// Store an operation, along with it's serialized representation.
    pub fn store_operation(&self, operation: WrappedOperation) {
        massa_trace!("storage.storage.store_operation", {
            "operation_id": operation.id
        });
        let mut operations = self.operations.write();
        match operations.entry(operation.id) {
            Entry::Occupied(_) => {}
            Entry::Vacant(entry) => {
                entry.insert(operation);
            }
        }
    }

    /// Returns a set of operation ids that are found in storage.
    pub fn find_operations(&self, operation_ids: Set<OperationId>) -> Set<OperationId> {
        let operations = self.operations.read();
        operation_ids
            .into_iter()
            .filter(|id| operations.contains_key(id))
            .collect()
    }

    /// Get a clone of the potentially stored operation.
    pub fn retrieve_operation(&self, operation_id: &OperationId) -> Option<WrappedOperation> {
        massa_trace!("storage.storage.retrieve_operation", {
            "operation_id": operation_id
        });
        let operations = self.operations.read();
        operations.get(operation_id).cloned()
    }

    /// Run a closure over a reference to a potentially stored operation.
    pub fn with_operation<F, V>(&self, operation_id: &OperationId, f: F) -> V
    where
        F: FnOnce(&Option<&WrappedOperation>) -> V,
    {
        massa_trace!("storage.storage.with_operation", {
            "operation_id": operation_id
        });
        let operations = self.operations.read();
        f(&operations.get(operation_id))
    }

    /// Run a closure over a list of references to potentially stored serialized operations.
    pub fn with_operations<F, V>(&self, operation_ids: &[OperationId], f: F) -> V
    where
        F: FnOnce(&[Option<&WrappedOperation>]) -> V,
    {
        massa_trace!("storage.storage.with_operations", {
            "operation_ids": operation_ids
        });
        let operations = self.operations.read();
        let results: Vec<Option<&WrappedOperation>> =
            operation_ids.iter().map(|id| operations.get(id)).collect();
        f(&results)
    }

    /// Remove a list of operations from storage.
    pub fn remove_operations(&self, operation_ids: &[OperationId]) {
        massa_trace!("storage.storage.remove_operation", {
            "operation_ids": operation_ids
        });
        let mut operations = self.operations.write();
        for id in operation_ids {
            operations.remove(id);
        }
    }
}
