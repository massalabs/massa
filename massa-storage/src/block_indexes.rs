use std::{collections::hash_map, collections::HashMap};

use massa_models::{
    prehash::{Map, Set},
    Address, BlockId, Slot, WrappedBlock,
};

/// Container for all blocks and different indexes.
/// Note: The structure can evolve and store more indexes.
#[derive(Default)]
pub struct BlockIndexes {
    /// Blocks structure container
    blocks: Map<BlockId, WrappedBlock>,
    /// Structure mapping creators with the created blocks
    index_by_creator: Map<Address, Set<BlockId>>,
    /// Structure mapping slot with their block id
    index_by_slot: HashMap<Slot, Set<BlockId>>,
}

impl BlockIndexes {
    /// Insert a block and populate the indexes.
    /// Arguments:
    /// - block: the block to insert
    pub(crate) fn insert(&mut self, block: WrappedBlock) {
        if let Ok(b) = self.blocks.try_insert(block.id, block) {
            // update creator index
            self.index_by_creator
                .entry(b.creator_address)
                .or_default()
                .insert(b.id);

            // update slot index
            self.index_by_slot
                .entry(b.content.header.content.slot)
                .or_default()
                .insert(b.id);
        }
    }

    /// Remove a block, remove from the indexes and do some clean-up in indexes if necessary.
    /// Arguments:
    /// - block_id: the block id to remove
    pub(crate) fn remove(&mut self, block_id: &BlockId) -> Option<WrappedBlock> {
        if let Some(b) = self.blocks.remove(block_id) {
            // update creator index
            if let hash_map::Entry::Occupied(mut occ) =
                self.index_by_creator.entry(b.creator_address)
            {
                occ.get_mut().remove(&b.id);
                if occ.get().is_empty() {
                    occ.remove();
                }
            }

            // update slot index
            if let hash_map::Entry::Occupied(mut occ) =
                self.index_by_slot.entry(b.content.header.content.slot)
            {
                occ.get_mut().remove(&b.id);
                if occ.get().is_empty() {
                    occ.remove();
                }
            }

            return Some(b);
        }
        None
    }

    /// Get a block reference by its ID
    /// Arguments:
    /// - id: ID of the block to retrieve
    ///
    /// Returns:
    /// - a reference to the block, or None if not found
    pub fn get(&self, id: &BlockId) -> Option<&WrappedBlock> {
        self.blocks.get(id)
    }

    /// Checks whether a block exists in global storage.
    pub fn contains(&self, id: &BlockId) -> bool {
        self.blocks.contains_key(id)
    }

    /// Get the block ids created by an address.
    /// Arguments:
    /// - address: the address to get the blocks created by
    ///
    /// Returns:
    /// - a reference to the block ids created by the address
    pub fn get_blocks_created_by(&self, address: &Address) -> Option<&Set<BlockId>> {
        self.index_by_creator.get(address)
    }

    /// Get the block id of the block at a slot.
    /// Arguments:
    /// - slot: the slot to get the block id of
    ///
    /// Returns:
    /// - the block id of the block at the slot if exists, None otherwise
    pub fn get_blocks_by_slot(&self, slot: &Slot) -> Option<&Set<BlockId>> {
        self.index_by_slot.get(slot)
    }
}
