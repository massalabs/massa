use std::{collections::HashMap, sync::Arc};

use massa_models::{
    prehash::{Map, Set},
    Address, BlockId, Slot, WrappedBlock,
};
use parking_lot::RwLock;

/// Container for all blocks and different indexes.
/// Note: The structure can evolve and store more indexes.
#[derive(Default)]
pub struct BlockIndexes {
    /// Blocks structure container
    pub(crate) blocks: Map<BlockId, Arc<RwLock<WrappedBlock>>>,
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
        let id = block.id;
        let creator = block.creator_address;
        self.index_by_creator.entry(creator).or_default().insert(id);
        self.index_by_slot.entry(block.content.header.content.slot).or_default().insert(block.id);
        self.blocks
            .entry(id)
            .or_insert(Arc::new(RwLock::new(block)));
    }

    /// Remove a block, remove from the indexes and made some clean-up in indexes if necessary.
    /// Arguments:
    /// - block_id: the block id to remove
    pub(crate) fn remove(&mut self, block_id: &BlockId) {
        let block = self
            .blocks
            .remove(&block_id)
            .expect("removing absent object from storage");
        let creator = block.read().creator_address;
        let slot = block.read().content.header.content.slot;
        let entry = self.index_by_creator.entry(creator).or_default();
        entry.remove(&block_id);
        if entry.is_empty() {
            self.index_by_creator.remove(&creator);
        }
        self.index_by_slot.remove(&slot);
    }

    /// Get the block ids created by an address.
    /// Arguments:
    /// - address: the address to get the blocks created by
    ///
    /// Returns:
    /// - the block ids created by the address
    pub fn get_blocks_created_by(&self, address: &Address) -> Vec<BlockId> {
        match self.index_by_creator.get(address) {
            Some(blocks) => blocks.iter().cloned().collect(),
            None => Vec::new(),
        }
    }

    /// Get the block id of the block at a slot.
    /// Arguments:
    /// - slot: the slot to get the block id of
    ///
    /// Returns:
    /// - the block id of the block at the slot if exists, None otherwise
    pub fn get_block_by_slot(&self, slot: Slot) -> Option<&Set<BlockId>> {
        self.index_by_slot.get(&slot)
    }
}
