//! Copyright (c) 2022 MASSA LABS <info@massa.net>
//!
//! This crate is used to store shared objects (blocks, operations...) across different modules.
//! The clonable `Storage` structure has thread-safe shared access to the stored objects.
//!
//! The `Storage` structure also has lists of object references held by the current instance of `Storage`.
//! When no instance of `Storage` claims a reference to a given object anymore, that object is automatically removed from storage.

#![warn(missing_docs)]
#![feature(hash_drain_filter)]
#![feature(map_try_insert)]

mod block_indexes;
mod denunciation_indexes;
mod endorsement_indexes;
mod operation_indexes;

#[cfg(test)]
mod tests;

use block_indexes::BlockIndexes;
use denunciation_indexes::DenunciationIndexes;
use endorsement_indexes::EndorsementIndexes;
use massa_models::denunciation::Denunciation;
use massa_models::prehash::{CapacityAllocator, PreHashMap, PreHashSet, PreHashed};
use massa_models::secure_share::Id;
use massa_models::{
    block::SecureShareBlock,
    block_id::BlockId,
    denunciation::DenunciationId,
    endorsement::{EndorsementId, SecureShareEndorsement},
    operation::{OperationId, SecureShareOperation},
};
use operation_indexes::OperationIndexes;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::fmt::Debug;
use std::hash::Hash;
use std::{collections::hash_map, sync::Arc};

/// A storage system for objects (blocks, operations...), shared by various components.
pub struct Storage {
    /// global block storage
    blocks: Arc<RwLock<BlockIndexes>>,
    /// global operation storage
    operations: Arc<RwLock<OperationIndexes>>,
    /// global operation storage
    endorsements: Arc<RwLock<EndorsementIndexes>>,
    /// global denunciation storage
    denunciations: Arc<RwLock<DenunciationIndexes>>,

    /// global block reference counter
    block_owners: Arc<RwLock<PreHashMap<BlockId, usize>>>,
    /// global operation reference counter
    operation_owners: Arc<RwLock<PreHashMap<OperationId, usize>>>,
    /// global endorsement reference counter
    endorsement_owners: Arc<RwLock<PreHashMap<EndorsementId, usize>>>,
    /// global denunciation reference counter
    denunciation_owners: Arc<RwLock<PreHashMap<DenunciationId, usize>>>,

    /// locally used block references
    local_used_blocks: PreHashSet<BlockId>,
    /// locally used operation references
    local_used_ops: PreHashSet<OperationId>,
    /// locally used endorsement references
    local_used_endorsements: PreHashSet<EndorsementId>,
    /// locally used denunciation references
    local_used_denunciations: PreHashSet<DenunciationId>,
}

impl Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO format storage
        f.write_str("")
    }
}

impl Clone for Storage {
    fn clone(&self) -> Self {
        let mut res = Self::clone_without_refs(self);

        // claim one more user of the op refs
        Storage::internal_claim_refs(
            &self.local_used_ops.clone(),
            &mut res.operation_owners.write(),
            &mut res.local_used_ops,
        );

        // claim one more user of the block refs
        Storage::internal_claim_refs(
            &self.local_used_blocks.clone(),
            &mut res.block_owners.write(),
            &mut res.local_used_blocks,
        );

        // claim one more user of the endorsement refs
        Storage::internal_claim_refs(
            &self.local_used_endorsements.clone(),
            &mut res.endorsement_owners.write(),
            &mut res.local_used_endorsements,
        );

        res
    }
}

impl Storage {
    /// Creates a new `Storage` instance. Must be called only one time in the execution:
    /// - In the main for the node
    /// - At the top of the test in tests
    /// All others instances of Storage must be cloned from this one using `clone()` or `clone_without_refs()`.
    pub fn create_root() -> Storage {
        Storage {
            blocks: Default::default(),
            operations: Default::default(),
            endorsements: Default::default(),
            denunciations: Default::default(),
            block_owners: Default::default(),
            operation_owners: Default::default(),
            endorsement_owners: Default::default(),
            denunciation_owners: Default::default(),
            local_used_blocks: Default::default(),
            local_used_ops: Default::default(),
            local_used_endorsements: Default::default(),
            local_used_denunciations: Default::default(),
        }
    }

    /// Clones the object to a new one that has no references
    pub fn clone_without_refs(&self) -> Self {
        Self {
            blocks: self.blocks.clone(),
            operations: self.operations.clone(),
            endorsements: self.endorsements.clone(),
            denunciations: self.denunciations.clone(),

            operation_owners: self.operation_owners.clone(),
            block_owners: self.block_owners.clone(),
            endorsement_owners: self.endorsement_owners.clone(),
            denunciation_owners: self.denunciation_owners.clone(),

            // do not clone local ref lists
            local_used_ops: Default::default(),
            local_used_blocks: Default::default(),
            local_used_endorsements: Default::default(),
            local_used_denunciations: Default::default(),
        }
    }

    /// Efficiently extends the current Storage by consuming the refs of another storage.
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
        blocks: &PreHashSet<BlockId>,
        operations: &PreHashSet<OperationId>,
        endorsements: &PreHashSet<EndorsementId>,
        denunciations: &PreHashSet<DenunciationId>,
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

        res.local_used_denunciations = denunciations
            .iter()
            .map(|id| {
                self.local_used_denunciations
                    .take(id)
                    .expect("split endorsement ref not owned by source")
            })
            .collect();

        res
    }

    /// internal helper to locally claim a reference to an object
    fn internal_claim_refs<IdT: Id + PartialEq + Eq + Hash + PreHashed + Copy>(
        ids: &PreHashSet<IdT>,
        owners: &mut RwLockWriteGuard<PreHashMap<IdT, usize>>,
        local_used_ids: &mut PreHashSet<IdT>,
    ) {
        for &id in ids {
            if local_used_ids.insert(id) {
                owners.entry(id).and_modify(|v| *v += 1).or_insert(1);
            }
        }
    }

    /// get the block reference ownership
    pub fn get_block_refs(&self) -> &PreHashSet<BlockId> {
        &self.local_used_blocks
    }

    /// Claim block references.
    /// Returns the set of block refs that were found and claimed.
    pub fn claim_block_refs(&mut self, ids: &PreHashSet<BlockId>) -> PreHashSet<BlockId> {
        let mut claimed = PreHashSet::with_capacity(ids.len());

        if ids.is_empty() {
            return claimed;
        }

        let owners = &mut self.block_owners.write();

        // check that all IDs are owned
        claimed.extend(ids.iter().filter(|id| owners.contains_key(id)));

        // effectively add local ownership on the refs
        Storage::internal_claim_refs(&claimed, owners, &mut self.local_used_blocks);

        claimed
    }

    /// Drop block references
    pub fn drop_block_refs(&mut self, ids: &PreHashSet<BlockId>) {
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
            for b_id in orphaned_ids {
                blocks.remove(&b_id);
            }
        }
    }

    /// Store a block
    /// Note that this also claims a local reference to the block
    pub fn store_block(&mut self, block: SecureShareBlock) {
        let id = block.id;
        let mut owners = self.block_owners.write();
        let mut blocks = self.blocks.write();
        blocks.insert(block);
        // update local reference counters
        Storage::internal_claim_refs(
            &vec![id].into_iter().collect(),
            &mut owners,
            &mut self.local_used_blocks,
        );
    }

    /// Claim operation references.
    /// Returns the set of operation refs that were found and claimed.
    pub fn claim_operation_refs(
        &mut self,
        ids: &PreHashSet<OperationId>,
    ) -> PreHashSet<OperationId> {
        let mut claimed = PreHashSet::with_capacity(ids.len());

        if ids.is_empty() {
            return claimed;
        }

        let owners = &mut self.operation_owners.write();

        // check that all IDs are owned
        claimed.extend(ids.iter().filter(|id| owners.contains_key(id)));

        // effectively add local ownership on the refs
        Storage::internal_claim_refs(&claimed, owners, &mut self.local_used_ops);

        claimed
    }

    /// get the operation reference ownership
    pub fn get_op_refs(&self) -> &PreHashSet<OperationId> {
        &self.local_used_ops
    }

    /// Drop local operation references.
    /// Ignores already-absent refs.
    pub fn drop_operation_refs(&mut self, ids: &PreHashSet<OperationId>) {
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
                ops.remove(&id);
            }
        }
    }

    /// Store operations
    /// Claims a local reference to the added operation
    pub fn store_operations(&mut self, operations: Vec<SecureShareOperation>) {
        if operations.is_empty() {
            return;
        }
        let mut owners = self.operation_owners.write();
        let mut op_store = self.operations.write();
        let ids: PreHashSet<OperationId> = operations.iter().map(|op| op.id).collect();
        for op in operations {
            op_store.insert(op);
        }
        Storage::internal_claim_refs(&ids, &mut owners, &mut self.local_used_ops);
    }

    /// Gets a read reference to the operations index
    pub fn read_operations(&self) -> RwLockReadGuard<OperationIndexes> {
        self.operations.read()
    }

    /// Gets a read reference to the endorsements index
    pub fn read_endorsements(&self) -> RwLockReadGuard<EndorsementIndexes> {
        self.endorsements.read()
    }

    /// Gets a read reference to the blocks index
    pub fn read_blocks(&self) -> RwLockReadGuard<BlockIndexes> {
        self.blocks.read()
    }

    /// Gets a read reference to the denunciation index
    pub fn read_denunciations(&self) -> RwLockReadGuard<DenunciationIndexes> {
        self.denunciations.read()
    }

    /// Claim endorsement references.
    /// Returns the set of operation refs that were found and claimed.
    pub fn claim_endorsement_refs(
        &mut self,
        ids: &PreHashSet<EndorsementId>,
    ) -> PreHashSet<EndorsementId> {
        let mut claimed = PreHashSet::with_capacity(ids.len());

        if ids.is_empty() {
            return claimed;
        }

        let owners = &mut self.endorsement_owners.write();

        // check that all IDs are owned
        claimed.extend(ids.iter().filter(|id| owners.contains_key(id)));

        // effectively add local ownership on the refs
        Storage::internal_claim_refs(&claimed, owners, &mut self.local_used_endorsements);
        claimed
    }

    /// get the endorsement reference ownership
    pub fn get_endorsement_refs(&self) -> &PreHashSet<EndorsementId> {
        &self.local_used_endorsements
    }

    /// Drop local endorsement references.
    /// Ignores already-absent refs.
    pub fn drop_endorsement_refs(&mut self, ids: &PreHashSet<EndorsementId>) {
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
                endos.remove(&id);
            }
        }
    }

    /// Store endorsements
    /// Claims local references to the added endorsements
    pub fn store_endorsements(&mut self, endorsements: Vec<SecureShareEndorsement>) {
        if endorsements.is_empty() {
            return;
        }
        let mut owners = self.endorsement_owners.write();
        let mut endo_store = self.endorsements.write();
        let ids: PreHashSet<EndorsementId> = endorsements.iter().map(|op| op.id).collect();
        for endorsement in endorsements {
            endo_store.insert(endorsement);
        }
        Storage::internal_claim_refs(&ids, &mut owners, &mut self.local_used_endorsements);
    }

    /// Store denunciation
    pub fn store_denunciation(&mut self, denunciation: Denunciation) {
        let mut owners = self.denunciation_owners.write();
        let mut de_indexes = self.denunciations.write();
        let de_id = DenunciationId::from(&denunciation);
        de_indexes.insert(denunciation);
        let mut ids = PreHashSet::with_capacity(1);
        ids.insert(de_id);
        Storage::internal_claim_refs(&ids, &mut owners, &mut self.local_used_denunciations);
    }

    /// get the denunciation reference ownership
    pub fn get_denunciation_refs(&self) -> &PreHashSet<DenunciationId> {
        &self.local_used_denunciations
    }

    /// Drop local denunciation references.
    /// Ignores already-absent refs.
    pub fn drop_denunciation_refs(&mut self, ids: &PreHashSet<DenunciationId>) {
        if ids.is_empty() {
            return;
        }
        let mut owners = self.denunciation_owners.write();
        let mut orphaned_ids = Vec::new();
        for id in ids {
            if !self.local_used_denunciations.remove(id) {
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
            let mut des = self.denunciations.write();
            for id in orphaned_ids {
                des.remove(&id);
            }
        }
    }
}

impl Drop for Storage {
    /// cleanup on Storage instance drop
    fn drop(&mut self) {
        // release all blocks
        self.drop_block_refs(&self.local_used_blocks.clone());

        // release all ops
        self.drop_operation_refs(&self.local_used_ops.clone());

        // release all endorsements
        self.drop_endorsement_refs(&self.local_used_endorsements.clone());
    }
}
