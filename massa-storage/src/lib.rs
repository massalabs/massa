//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//!
//! This crate is used to store shared objects (blocks, operations...) across different modules.
//! The clonable `Storage` module has thread-safe shared access to the stored objects.
//!
//! The `Storage` module also has lists of object references held by the current instance of `Storage`.
//! When no instance of `Storage` claims a reference to a given object anymore, that object is automatically removed from storage.

#![warn(missing_docs)]

use massa_logging::massa_trace;
use massa_models::prehash::{Map, PreHashed, Set};
use massa_models::wrapped::Id;
use massa_models::{BlockId, OperationId, WrappedBlock, WrappedOperation};
use parking_lot::{RwLock, RwLockWriteGuard};
use std::hash::Hash;
use std::{collections::hash_map, sync::Arc};

/// A storage system for objects (blocks, operations...), shared by various components.
#[derive(Default)]
pub struct Storage {
    /// global block storage
    blocks: Arc<RwLock<Map<BlockId, Arc<RwLock<WrappedBlock>>>>>,
    /// global operation storage
    operations: Arc<RwLock<Map<OperationId, WrappedOperation>>>,

    /// global block reference counter
    block_owners: Arc<RwLock<Map<BlockId, usize>>>,
    /// global operation reference counter
    operation_owners: Arc<RwLock<Map<OperationId, usize>>>,

    /// locally used block references
    local_used_blocks: Set<BlockId>,
    /// locally used block references
    local_used_ops: Set<OperationId>,
}

impl Clone for Storage {
    /// Clones the Storage instance
    /// Note that the local references are reset for the new instance.
    fn clone(&self) -> Self {
        Self {
            blocks: self.blocks.clone(),
            operations: self.operations.clone(),
            operation_owners: self.operation_owners.clone(),
            block_owners: self.block_owners.clone(),

            // local reference lists are not cloned
            local_used_ops: Default::default(),
            local_used_blocks: Default::default(),
        }
    }
}

impl Storage {
    /// internal helper to locally claim a reference to an object
    fn internal_claim_refs<IdT: Id + PartialEq + Eq + Hash + PreHashed + Copy>(
        ids: &[IdT],
        mut owners: RwLockWriteGuard<Map<IdT, usize>>,
        local_used_ids: &mut Set<IdT>,
    ) {
        for &id in ids {
            if local_used_ids.insert(id) {
                owners.entry(id).and_modify(|v| *v += 1).or_insert(1);
            }
        }
    }

    /// Claim block references for the current module
    pub fn claim_block_refs(&mut self, ids: &[BlockId]) {
        if ids.is_empty() {
            return;
        }
        Storage::internal_claim_refs(ids, self.block_owners.write(), &mut self.local_used_blocks);
    }

    /// Drop block references in the current module
    pub fn drop_block_refs(&mut self, ids: &[BlockId]) {
        if ids.is_empty() {
            return;
        }
        let mut owners = self.block_owners.write();
        let mut orphaned_ids = Vec::new();
        for id in ids {
            if !self.local_used_blocks.remove(id) {
                // the object was already not referenced locally
                continue;
            }
            match owners.entry(*id) {
                hash_map::Entry::Occupied(mut occ) => {
                    let res_count = {
                        let cnt = occ.get_mut();
                        *cnt = cnt
                            .checked_sub(1)
                            .expect("less than 1 owner on storage object reference drop");
                        *cnt
                    };
                    if res_count == 0 {
                        orphaned_ids.push(*id);
                        occ.remove();
                    }
                }
                hash_map::Entry::Vacant(_vac) => {
                    panic!("missing object in storage on storage object reference drop");
                }
            }
        }
        // if there are orphaned objects, remove them from storage
        if !orphaned_ids.is_empty() {
            let mut blocks = self.blocks.write();
            for id in orphaned_ids {
                if blocks.remove(&id).is_none() {
                    panic!("removing absent object from storage")
                }
            }
        }
    }

    /// Store a block
    /// Note that this also claims a local reference to the block
    pub fn store_block(&mut self, block: WrappedBlock) {
        massa_trace!("storage.storage.store_block", { "block_id": block.id });
        let id = block.id;
        let mut blocks = self.blocks.write();
        let owners = self.block_owners.write();
        // insert block
        blocks
            .entry(id)
            .or_insert_with(|| Arc::new(RwLock::new(block)));
        // update local reference counters
        Storage::internal_claim_refs(&vec![id], owners, &mut self.local_used_blocks);
    }

    /// Get a (mutable) reference to a stored block.
    pub fn retrieve_block(&self, block_id: &BlockId) -> Option<Arc<RwLock<WrappedBlock>>> {
        massa_trace!("storage.storage.retrieve_block", { "block_id": block_id });
        self.blocks.read().get(block_id).map(Arc::clone)
    }

    /// Claim operation references
    pub fn claim_operation_refs(&mut self, ids: &[OperationId]) {
        if ids.is_empty() {
            return;
        }
        Storage::internal_claim_refs(ids, self.operation_owners.write(), &mut self.local_used_ops);
    }

    /// Drop local operation references
    pub fn drop_operation_refs(&mut self, ids: &[OperationId]) {
        if ids.is_empty() {
            return;
        }
        let mut owners = self.operation_owners.write();
        let mut orphaned_ids = Vec::new();
        for id in ids {
            if !self.local_used_ops.remove(id) {
                // the object was already not referenced locally
                continue;
            }
            match owners.entry(*id) {
                hash_map::Entry::Occupied(mut occ) => {
                    let res_count = {
                        let cnt = occ.get_mut();
                        *cnt = cnt
                            .checked_sub(1)
                            .expect("less than 1 owner on storage object reference drop");
                        *cnt
                    };
                    if res_count == 0 {
                        orphaned_ids.push(*id);
                        occ.remove();
                    }
                }
                hash_map::Entry::Vacant(_vac) => {
                    panic!("missing object in storage on storage object reference drop");
                }
            }
        }
        // if there are orphaned objects, remove them from storage
        if !orphaned_ids.is_empty() {
            let mut ops = self.operations.write();
            for id in orphaned_ids {
                if ops.remove(&id).is_none() {
                    panic!("removing absent object from storage")
                }
            }
        }
    }

    /// Store operations
    /// Claims a local reference to the added operation
    pub fn store_operations(&mut self, operations: Vec<WrappedOperation>) {
        if operations.is_empty() {
            return;
        }
        let mut op_store = self.operations.write();
        let owners = self.operation_owners.write();
        let ids: Vec<OperationId> = operations.iter().map(|op| op.id).collect();
        for op in operations {
            op_store.entry(op.id).or_insert(op);
        }
        Storage::internal_claim_refs(&ids, owners, &mut self.local_used_ops);
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
}

impl Drop for Storage {
    /// cleanup on Storage instance drop
    fn drop(&mut self) {
        // release all blocks
        self.drop_block_refs(&self.local_used_blocks.iter().copied().collect::<Vec<_>>());

        // release all ops
        self.drop_operation_refs(&self.local_used_ops.iter().copied().collect::<Vec<_>>());
    }
}
