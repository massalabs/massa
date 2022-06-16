//! This crate is used to share blocks across the node

#![warn(missing_docs)]

use massa_logging::massa_trace;
use massa_models::operation::{OperationIds, OperationPrefixId, OperationSuffixId};
use massa_models::prehash::Map;
use massa_models::{Block, BlockId, OperationId, SignedOperation};
use parking_lot::RwLock;
use std::collections::hash_map::Entry;
use std::sync::Arc;
use tracing::warn;

/// Stored block: block + serialized block + (serialized header)
#[derive(Debug)]
pub struct StoredBlock {
    /// The block.
    pub block: Block,
    /// The serialized representation of the block.
    pub serialized: Vec<u8>,
    /// The serialized representation of the header, if any.
    /// Note: the header is written as part of propagation of headers.
    pub serialized_header: Option<Vec<u8>>,
}

/// Stored operation + serialized operation
#[derive(Clone, Debug)]
pub struct StoredOperation {
    /// The operation.
    pub operation: SignedOperation,
    /// The serialized representation of the operation.
    pub serialized: Vec<u8>,
}

/// A storage of block, shared by various components.
#[derive(Clone, Default)]
pub struct Storage {
    blocks: Arc<RwLock<Map<BlockId, Arc<RwLock<StoredBlock>>>>>,
    operations: Arc<RwLock<Map<OperationPrefixId, Map<OperationSuffixId, StoredOperation>>>>,
}

impl Storage {
    /// Store a block, along with it's serialized representation.
    pub fn store_block(&self, block_id: BlockId, block: Block, serialized: Vec<u8>) {
        massa_trace!("storage.storage.store_block", { "block_id": block_id });
        let mut blocks = self.blocks.write();
        match blocks.entry(block_id) {
            Entry::Occupied(_) => {}
            Entry::Vacant(entry) => {
                let stored_block = StoredBlock {
                    block,
                    serialized,
                    serialized_header: None,
                };
                let to_store = Arc::new(RwLock::new(stored_block));
                entry.insert(to_store);
            }
        }
    }

    /// Get a (mutable) reference to the stored block.
    pub fn retrieve_block(&self, block_id: &BlockId) -> Option<Arc<RwLock<StoredBlock>>> {
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
    pub fn store_operation(
        &self,
        operation_id: OperationId,
        operation: SignedOperation,
        serialized: Vec<u8>,
    ) {
        massa_trace!("storage.storage.store_operation", {
            "operation_id": operation_id
        });
        let mut operations = self.operations.write();
        let (prefix, suffix) = operation_id.into_split();
        let stored_operation = StoredOperation {
            operation,
            serialized,
        };
        match operations.entry(prefix) {
            Entry::Occupied(mut entry) => match entry.get_mut().entry(suffix) {
                Entry::Occupied(_) => {}
                Entry::Vacant(entry) => {
                    entry.insert(stored_operation);
                }
            },
            Entry::Vacant(entry) => {
                let mut m = Map::default();
                m.insert(suffix, stored_operation);
                entry.insert(m);
            }
        }
    }

    /// Get a clone of the potentially stored operation.
    pub fn retrieve_operation(&self, operation_id: &OperationId) -> Option<StoredOperation> {
        massa_trace!("storage.storage.retrieve_operation", {
            "operation_id": operation_id
        });
        let operations = self.operations.read();
        let (prefix, suffix) = operation_id.split();
        match operations.get(&prefix) {
            Some(m) => m.get(&suffix).cloned(),
            _ => None,
        }
    }

    /// Get a list of operation ids prefixed with `prefix` argument.
    pub fn retrieve_operations_prefixed(&self, prefix: &OperationPrefixId) -> Option<OperationIds> {
        massa_trace!("storage.storage.retrieve_operations_prefixed", {
            "prefix": prefix
        });
        let operations = self.operations.read();
        match operations.get(prefix) {
            Some(m) => {
                let mut ret = OperationIds::default();
                for (suffix, _) in m.iter() {
                    match prefix.join(suffix) {
                        Ok(id) => ret.insert(id),
                        Err(e) => {
                            // ignore the failure and print a warning
                            warn!("problem occurs on try retreive an operation id {}", e);
                            continue;
                        }
                    };
                }
                Some(ret)
            }
            _ => None,
        }
    }

    /// Run a closure over a reference to a potentially stored operation.
    pub fn with_operation<F, V>(&self, operation_id: &OperationId, f: F) -> V
    where
        F: FnOnce(&Option<&StoredOperation>) -> V,
    {
        massa_trace!("storage.storage.with_operation", {
            "operation_id": operation_id
        });
        let operations = self.operations.read();
        let (prefix, suffix) = operation_id.split();
        f(&match operations.get(&prefix) {
            Some(m) => m.get(&suffix),
            _ => None,
        })
    }

    /// Run a closure over a list of references to potentially stored serialized operations.
    pub fn with_serialized_operations<F, V>(&self, operation_ids: &[OperationId], f: F) -> V
    where
        F: FnOnce(&[Option<&Vec<u8>>]) -> V,
    {
        massa_trace!("storage.storage.with_serialized_operations", {
            "operation_ids": operation_ids
        });
        let operations = self.operations.read();
        let results: Vec<Option<&Vec<u8>>> = operation_ids
            .iter()
            .map(|id| {
                let (prefix, suffix) = id.split();
                match operations.get(&prefix) {
                    Some(m) => m.get(&suffix),
                    _ => None,
                }
                .map(|stored| &stored.serialized)
            })
            .collect();
        f(&results)
    }

    /// Remove a list of operations from storage.
    pub fn remove_operations(&self, operation_ids: &[OperationId]) {
        massa_trace!("storage.storage.remove_operation", {
            "operation_ids": operation_ids
        });
        let mut operations = self.operations.write();
        for id in operation_ids {
            let (prefix, suffix) = id.split();
            let len = match operations.get_mut(&prefix) {
                Some(m) => {
                    m.remove(&suffix);
                    m.len()
                }
                _ => return,
            };
            if len == 0 {
                operations.remove(&prefix);
            }
        }
    }
}
