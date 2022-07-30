use massa_models::prehash::{Map, PreHashed, Set};
use massa_models::wrapped::Id;
use massa_models::{BlockId, OperationId};
use parking_lot::RwLockWriteGuard;
use std::collections::hash_map::{self};
use std::hash::Hash;

use crate::Storage;

/// Structure listing which (operations, blocks etc...) in Storage are in use by a given module.
/// When no modules use a given object anymore, it is automatically deleted from Storage.
/// A given module can claim only one usage token per object.
pub struct ModuleStorageUsage {
    /// set of used operation IDs
    op_ids: Set<OperationId>,

    /// set of used block IDs
    block_ids: Set<BlockId>,

    /// reference to the Storage module
    storage: Storage,
}

impl ModuleStorageUsage {
    /// Claim that the module now uses a list of objects.
    /// If the module was already decleared as using some of those objects, they are ignored.
    /// /// Internal helper function.
    fn use_objs<IdT>(
        obj_ids: &[IdT],
        mut obj_owners: RwLockWriteGuard<Map<IdT, usize>>,
        self_obj_ids: &mut Set<IdT>,
    ) where
        IdT: Id + PreHashed + PartialEq + Eq + Hash + Copy,
    {
        for &id in obj_ids {
            if !self_obj_ids.insert(id) {
                continue;
            }
            match obj_owners.entry(id) {
                hash_map::Entry::Occupied(mut occ) => {
                    let val = occ.get_mut();
                    *val = *val + 1;
                }
                hash_map::Entry::Vacant(vac) => {
                    vac.insert(1);
                }
            }
        }
    }

    /// Signal that the module does not use a list of objects anymore.
    /// If the module was already not using some of those objects, they are ignored.
    /// Internal helper function.
    fn release_objs<IdT, FnT>(
        obj_ids: &[IdT],
        mut obj_owners: RwLockWriteGuard<Map<IdT, usize>>,
        self_obj_ids: &mut Set<IdT>,
        remove_objs_from_storage: FnT,
    ) where
        IdT: Id + PreHashed + PartialEq + Eq + Hash + Copy,
        FnT: Fn(&[IdT]) -> (),
    {
        for id in obj_ids {
            if !self_obj_ids.remove(id) {
                continue;
            }
            match obj_owners.entry(*id) {
                hash_map::Entry::Occupied(mut occ) => {
                    let res_count = {
                        let cnt = occ.get_mut();
                        *cnt = cnt
                            .checked_sub(1)
                            .expect("less than 1 owner on op pointer drop");
                        *cnt
                    };
                    if res_count == 0 {
                        remove_objs_from_storage(&[*id]);
                        occ.remove();
                    }
                }
                hash_map::Entry::Vacant(_vac) => {
                    panic!("missing operation in storage on pointer drop");
                }
            }
        }
    }

    /// Claim that the module now uses a list of operations.
    /// If the module was already decleared as using some of those ops, they are ignored.
    pub fn use_ops(&mut self, ids: &[OperationId]) {
        ModuleStorageUsage::use_objs(ids, self.storage.operation_owners.write(), &mut self.op_ids);
    }

    /// Signal that the module does not use a list of operations anymore.
    /// If the module was already not using some of those ops, they are ignored.
    pub fn release_ops(&mut self, ids: &[OperationId]) {
        ModuleStorageUsage::release_objs(
            ids,
            self.storage.operation_owners.write(),
            &mut self.op_ids,
            |ids: &[_]| {
                self.storage.remove_operations(ids);
            },
        );
    }

    /// Check whether the module has declared that is uses a given operation.
    pub fn uses_op(&self, id: &OperationId) -> bool {
        self.op_ids.contains(id)
    }

    /// Claim that the module now uses a list of blocks.
    /// If the module was already decleared as using some of those blocks, they are ignored.
    pub fn use_blocks(&mut self, ids: &[BlockId]) {
        ModuleStorageUsage::use_objs(ids, self.storage.block_owners.write(), &mut self.block_ids);
    }

    /// Signal that the module does not use a list of blocks anymore.
    /// If the module was already not using some of those blocks, they are ignored.
    pub fn release_blocks(&mut self, ids: &[BlockId]) {
        ModuleStorageUsage::release_objs(
            ids,
            self.storage.block_owners.write(),
            &mut self.block_ids,
            |ids: &[_]| {
                self.storage.remove_blocks(ids);
            },
        );
    }

    /// Check whether the module has declared that is uses a given block.
    pub fn uses_block(&self, id: &BlockId) -> bool {
        self.block_ids.contains(id)
    }

    /// Create a new ModuleStorageUsage without any initial usages.
    pub fn new(storage: Storage) -> Self {
        Self {
            op_ids: Default::default(),
            block_ids: Default::default(),
            storage,
        }
    }
}

impl Drop for ModuleStorageUsage {
    /// Release all used objects when dropped
    fn drop(&mut self) {
        self.release_ops(&self.op_ids.iter().copied().collect::<Vec<_>>());
        self.release_blocks(&self.block_ids.iter().copied().collect::<Vec<_>>());
    }
}
