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
    index_by_slot: HashMap<Slot, BlockId>,
}

impl BlockIndexes {
    pub(crate) fn insert(&mut self, block: WrappedBlock) {
        let id = block.id;
        let creator = block.creator_address;
        self.index_by_creator.entry(creator).or_default().insert(id);
        self.index_by_slot
            .insert(block.content.header.content.slot, block.id);
        self.blocks
            .entry(id)
            .or_insert(Arc::new(RwLock::new(block)));
    }

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

    pub fn get_blocks_created_by(&self, address: &Address) -> Vec<WrappedBlock> {
        match self.index_by_creator.get(address) {
            Some(ids) => ids
                .iter()
                .filter_map(|id| Some(self.blocks.get(id).unwrap().read().clone()))
                .collect(),
            _ => return Vec::default(),
        }
    }

    pub fn get_block_by_slot(&self, slot: Slot) -> Option<WrappedBlock> {
        match self.index_by_slot.get(&slot) {
            Some(id) => Some(self.blocks.get(id).unwrap().read().clone()),
            _ => return None,
        }
    }
}
