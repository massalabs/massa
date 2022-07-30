use massa_logging::massa_trace;
use massa_models::prehash::{Map, Set};
use massa_models::{BlockId, OperationId, WrappedBlock, WrappedOperation};
use parking_lot::RwLock;
use std::sync::Arc;

/// A storage of block, shared by various components.
#[derive(Clone, Default)]
pub struct Storage {
    blocks: Arc<RwLock<Map<BlockId, Arc<RwLock<WrappedBlock>>>>>,
    operations: Arc<RwLock<Map<OperationId, WrappedOperation>>>,

    pub(crate) operation_owners: Arc<RwLock<Map<OperationId, usize>>>,
    pub(crate) block_owners: Arc<RwLock<Map<BlockId, usize>>>,
}

impl Storage {
    /// Store a block
    pub fn store_block(&self, block: WrappedBlock) {
        massa_trace!("storage.storage.store_block", { "block_id": block.id });
        self.blocks
            .write()
            .entry(block.id)
            .or_insert_with(|| Arc::new(RwLock::new(block)));
    }

    /// Get a (mutable) reference to a stored block.
    pub fn retrieve_block(&self, block_id: &BlockId) -> Option<Arc<RwLock<WrappedBlock>>> {
        massa_trace!("storage.storage.retrieve_block", { "block_id": block_id });
        self.blocks.read().get(block_id).map(Arc::clone)
    }

    /// Remove a list of blocks from storage.
    pub fn remove_blocks(&self, block_ids: &[BlockId]) {
        massa_trace!("storage.storage.remove_blocks", { "block_ids": block_ids });
        let mut blocks = self.blocks.write();
        for id in block_ids {
            blocks.remove(id);
        }
    }

    /// Store operations
    pub fn store_operations(&self, operations: Vec<WrappedOperation>) {
        let mut op_store = self.operations.write();
        for op in operations {
            op_store.entry(op.id).or_insert(op);
        }
    }

    /// Return a set of operation ids that are found in storage.
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
        self.operations.read().get(operation_id).cloned()
    }

    /// Run a closure over a reference to a potentially stored operation.
    pub fn with_operation<F, V>(&self, operation_id: &OperationId, f: F) -> V
    where
        F: FnOnce(&Option<&WrappedOperation>) -> V,
    {
        massa_trace!("storage.storage.with_operation", {
            "operation_id": operation_id
        });
        f(&self.operations.read().get(operation_id))
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
