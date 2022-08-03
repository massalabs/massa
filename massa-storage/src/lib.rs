//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//!
//! This crate is used to store shared objects (blocks, operations...) across different modules.
//! The clonable `Storage` struct has thread-safe shared access to the stored objects.
//!
//! The `Storage` struct also has lists of object references held by the current instance of `Storage`.
//! When no instance of `Storage` claims a reference to a given object anymore, that object is automatically removed from storage.

#![warn(missing_docs)]
#![feature(hash_drain_filter)]

use massa_logging::massa_trace;
use massa_models::prehash::{Map, PreHashed, Set};
use massa_models::wrapped::Id;
use massa_models::{
    BlockId, EndorsementId, OperationId, WrappedBlock, WrappedEndorsement, WrappedOperation,
};
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
    /// global operation storage
    endorsements: Arc<RwLock<Map<EndorsementId, WrappedEndorsement>>>,

    /// global block reference counter
    block_owners: Arc<RwLock<Map<BlockId, usize>>>,
    /// global operation reference counter
    operation_owners: Arc<RwLock<Map<OperationId, usize>>>,
    /// global endorsement reference counter
    endorsement_owners: Arc<RwLock<Map<EndorsementId, usize>>>,

    /// locally used block references
    local_used_blocks: Set<BlockId>,
    /// locally used operation references
    local_used_ops: Set<OperationId>,
    /// locally used endorsement references
    local_used_endorsements: Set<EndorsementId>,
}

impl Storage {
    /// Clones the object to a new one that has the same references
    pub fn clone_with_refs(&mut self) -> Self {
        // claim one more user of the op refs
        Storage::internal_claim_refs(
            &self.local_used_ops.clone(),
            &mut self.operation_owners.write(),
            &mut self.local_used_ops,
        );

        // claim one more user of the block refs
        Storage::internal_claim_refs(
            &self.local_used_blocks.clone(),
            &mut self.block_owners.write(),
            &mut self.local_used_blocks,
        );

        // claim one more user of the endorsement refs
        Storage::internal_claim_refs(
            &self.local_used_endorsements.clone(),
            &mut self.endorsement_owners.write(),
            &mut self.local_used_endorsements,
        );

        Self {
            blocks: self.blocks.clone(),
            operations: self.operations.clone(),
            endorsements: self.endorsements.clone(),
            operation_owners: self.operation_owners.clone(),
            block_owners: self.block_owners.clone(),
            endorsement_owners: self.endorsement_owners.clone(),

            local_used_ops: self.local_used_ops.clone(),
            local_used_blocks: self.local_used_blocks.clone(),
            local_used_endorsements: self.local_used_endorsements.clone(),
        }
    }

    /// Clones the object to a new one that has no references
    pub fn clone_without_refs(&self) -> Self {
        Self {
            blocks: self.blocks.clone(),
            operations: self.operations.clone(),
            endorsements: self.endorsements.clone(),
            operation_owners: self.operation_owners.clone(),
            block_owners: self.block_owners.clone(),
            endorsement_owners: self.endorsement_owners.clone(),

            // do not clone local ref lists
            local_used_ops: Default::default(),
            local_used_blocks: Default::default(),
            local_used_endorsements: Default::default(),
        }
    }

    /// Efficiently extends the current Storage by consuming the refs of another's.
    pub fn extend(&mut self, mut other: Storage) {
        // Take ownership ot `other`'s references.
        // Objects owned by both require a counter decrement and are handled when `other` is dropped.
        self.local_used_ops.extend(
            &other
                .local_used_ops
                .drain_filter(|id| !self.local_used_ops.contains(id))
                .collect::<Vec<_>>(),
        );

        self.local_used_blocks.extend(
            &other
                .local_used_blocks
                .drain_filter(|id| !self.local_used_blocks.contains(id))
                .collect::<Vec<_>>(),
        );

        self.local_used_endorsements.extend(
            &other
                .local_used_endorsements
                .drain_filter(|id| !self.local_used_endorsements.contains(id))
                .collect::<Vec<_>>(),
        );
    }

    /// Efficiently splits off a subset of the reference ownership into a new Storage object.
    /// Panics if some of the refs are not owned by the source.
    pub fn split_off(
        &mut self,
        blocks: &Set<BlockId>,
        operations: &Set<OperationId>,
        endorsements: &Set<EndorsementId>,
    ) -> Storage {
        // Make a clone of self, which has no ref ownership.
        let mut res = self.clone_without_refs();

        // Define the ref ownership of the new Storage as all the listed objects that we managed to remove from `self`.
        // Note that this does not require updating counters.

        res.local_used_blocks = blocks
            .iter()
            .map(|id| {
                self.local_used_blocks
                    .take(id)
                    .expect("split block ref not owned by source")
            })
            .collect();

        res.local_used_ops = operations
            .iter()
            .map(|id| {
                self.local_used_ops
                    .take(id)
                    .expect("split op ref not owned by source")
            })
            .collect();

        res.local_used_endorsements = endorsements
            .iter()
            .map(|id| {
                self.local_used_endorsements
                    .take(id)
                    .expect("split endorsement ref not owned by source")
            })
            .collect();

        res
    }

    /// internal helper to locally claim a reference to an object
    fn internal_claim_refs<IdT: Id + PartialEq + Eq + Hash + PreHashed + Copy>(
        ids: &Set<IdT>,
        owners: &mut RwLockWriteGuard<Map<IdT, usize>>,
        local_used_ids: &mut Set<IdT>,
    ) {
        for &id in ids {
            if local_used_ids.insert(id) {
                owners.entry(id).and_modify(|v| *v += 1).or_insert(1);
            }
        }
    }

    /// get the block reference ownership
    pub fn get_block_refs(&self) -> &Set<BlockId> {
        &self.local_used_blocks
    }

    /// Claim block references.
    /// Panics if some of the refs are not owned by the source.
    pub fn claim_block_refs(&mut self, source: &Storage, ids: &Set<BlockId>) {
        if ids.is_empty() {
            return;
        }
        if !ids.is_subset(&source.local_used_blocks) {
            panic!("some claimed blocks are not owned by source")
        }
        Storage::internal_claim_refs(
            &ids,
            &mut self.block_owners.write(),
            &mut self.local_used_blocks,
        );
    }

    /// Drop block references
    pub fn drop_block_refs(&mut self, ids: &Set<BlockId>) {
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
        let mut owners = self.block_owners.write();
        // insert block
        blocks
            .entry(id)
            .or_insert_with(|| Arc::new(RwLock::new(block)));
        // update local reference counters
        Storage::internal_claim_refs(
            &vec![id].into_iter().collect(),
            &mut owners,
            &mut self.local_used_blocks,
        );
    }

    /// Get a (mutable) reference to a stored block.
    pub fn retrieve_block(&self, block_id: &BlockId) -> Option<Arc<RwLock<WrappedBlock>>> {
        massa_trace!("storage.storage.retrieve_block", { "block_id": block_id });
        self.blocks.read().get(block_id).map(Arc::clone)
    }

    /// Claim operation references.
    /// Panics if some of the refs are not owned by the source.
    pub fn claim_operation_refs(&mut self, source: &Storage, ids: &Set<OperationId>) {
        if ids.is_empty() {
            return;
        }
        if !ids.is_subset(&source.local_used_ops) {
            panic!("some claimed ops are not owned by source")
        }
        Storage::internal_claim_refs(
            &ids,
            &mut self.operation_owners.write(),
            &mut self.local_used_ops,
        );
    }

    /// get the operation reference ownership
    pub fn get_op_refs(&self) -> &Set<OperationId> {
        &self.local_used_ops
    }

    /// Drop local operation references.
    /// Ignores already-absend refs.
    pub fn drop_operation_refs(&mut self, ids: &Set<OperationId>) {
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
        let mut owners = self.operation_owners.write();
        let ids: Set<OperationId> = operations.iter().map(|op| op.id).collect();
        for op in operations {
            op_store.entry(op.id).or_insert(op);
        }
        Storage::internal_claim_refs(&ids, &mut owners, &mut self.local_used_ops);
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

    /// Claim endorsement references.
    /// Panics if some of the refs are not owned by the source.
    pub fn claim_endorsement_refs(&mut self, source: &Storage, ids: &Set<EndorsementId>) {
        if ids.is_empty() {
            return;
        }
        if !ids.is_subset(&source.local_used_endorsements) {
            panic!("some claimed endorsements are not owned by source")
        }
        Storage::internal_claim_refs(
            &ids,
            &mut self.endorsement_owners.write(),
            &mut self.local_used_endorsements,
        );
    }

    /// get the endorsement reference ownership
    pub fn get_endorsement_refs(&self) -> &Set<EndorsementId> {
        &&self.local_used_endorsements
    }

    /// Drop local ndorsement references.
    /// Ignores already-absend refs.
    pub fn drop_endorsement_refs(&mut self, ids: &Set<EndorsementId>) {
        if ids.is_empty() {
            return;
        }
        let mut owners = self.endorsement_owners.write();
        let mut orphaned_ids = Vec::new();
        for id in ids {
            if !self.local_used_endorsements.remove(id) {
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
            let mut endos = self.endorsements.write();
            for id in orphaned_ids {
                if endos.remove(&id).is_none() {
                    panic!("removing absent object from storage")
                }
            }
        }
    }

    /// Store endorsements
    /// Claims local references to the added endorsements
    pub fn store_endorsements(&mut self, endorsements: Vec<WrappedEndorsement>) {
        if endorsements.is_empty() {
            return;
        }
        let mut endo_store = self.endorsements.write();
        let mut owners = self.endorsement_owners.write();
        let ids: Set<EndorsementId> = endorsements.iter().map(|op| op.id).collect();
        for endo in endorsements {
            endo_store.entry(endo.id).or_insert(endo);
        }
        Storage::internal_claim_refs(&ids, &mut owners, &mut self.local_used_endorsements);
    }
}

impl Drop for Storage {
    /// cleanup on Storage instance drop
    fn drop(&mut self) {
        // release all blocks
        self.drop_block_refs(&self.local_used_blocks.clone());

        // release all ops
        self.drop_operation_refs(&self.local_used_ops.clone());
    }
}
