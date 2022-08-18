use std::{collections::HashMap, sync::Arc};

use massa_models::{
    prehash::{Map, Set},
    Address, BlockId, EndorsementId, OperationId, Slot, WrappedBlock,
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
    /// Structure mapping operation id with ids of blocks they are contained in
    index_by_op: Map<OperationId, Set<BlockId>>,
    /// Structure mapping endorsement id with ids of blocks they are contained in
    index_by_endorsement: Map<EndorsementId, Set<BlockId>>,
}

impl BlockIndexes {
    /// Insert a block and populate the indexes.
    /// Arguments:
    /// - block: the block to insert
    pub(crate) fn insert(&mut self, block: WrappedBlock) {
        let id = block.id;
        let creator = block.creator_address;
        self.index_by_creator.entry(creator).or_default().insert(id);
        self.index_by_slot
            .entry(block.content.header.content.slot)
            .or_default()
            .insert(block.id);
        for id in block.content.operations.iter() {
            self.index_by_op
                .entry(*id)
                .or_default()
                .insert(block.id);
        }
        for endo in block.content.header.content.endorsements.iter() {
            self.index_by_endorsement
                .entry(endo.id)
                .or_default()
                .insert(block.id);
        }
        self.blocks
            .entry(id)
            .or_insert(Arc::new(RwLock::new(block)));
    }

    /// Remove a block, remove from the indexes and made some clean-up in indexes if necessary.
    /// Arguments:
    /// - block_id: the block id to remove
    pub(crate) fn remove(&mut self, block_id: &BlockId) {
        let stored_block = self
            .blocks
            .remove(block_id)
            .expect("removing absent object from storage");
        let block = stored_block.read();
        let creator = block.creator_address;
        let slot = block.content.header.content.slot;
        let creator_entry = self.index_by_creator.entry(creator).or_default();
        creator_entry.remove(block_id);
        if creator_entry.is_empty() {
            self.index_by_creator.remove(&creator);
        }
        let slot_entry = self.index_by_slot.entry(slot).or_default();
        slot_entry.remove(block_id);
        if slot_entry.is_empty() {
            self.index_by_slot.remove(&slot);
        }
        for id in block.content.operations.iter() {
            let op_entry = self.index_by_op.entry(*id).or_default();
            op_entry.remove(block_id);
            if op_entry.is_empty() {
                self.index_by_op.remove(id);
            }
        }
        for endo in block.content.header.content.endorsements.iter() {
            let endo_entry = self.index_by_endorsement.entry(endo.id).or_default();
            endo_entry.remove(block_id);
            if endo_entry.is_empty() {
                self.index_by_endorsement.remove(&endo.id);
            }
        }
    }

    /// Get the block ids created by an address.
    /// Arguments:
    /// - address: the address to get the blocks created by
    ///
    /// Returns:
    /// - the block ids created by the address
    pub fn get_blocks_created_by(&self, address: &Address) -> Set<BlockId> {
        match self.index_by_creator.get(address) {
            Some(ids) => ids.clone(),
            None => Set::default(),
        }
    }

    /// Get the block id of the block at a slot.
    /// Arguments:
    /// - slot: the slot to get the block id of
    ///
    /// Returns:
    /// - the block id of the block at the slot if exists, None otherwise
    pub fn get_blocks_by_slot(&self, slot: Slot) -> Option<&Set<BlockId>> {
        self.index_by_slot.get(&slot)
    }

    /// Get a list of block id containing an operation.
    /// Arguments:
    /// - operation_id: the operation id to get the block id of
    ///
    /// Returns:
    /// - the list of block id containing the operation
    pub fn get_blocks_by_op(&self, operation_id: &OperationId) -> Set<BlockId> {
        match self.index_by_op.get(operation_id) {
            Some(blocks) => blocks.clone(),
            None => Set::default(),
        }
    }

    /// Get a list of block id containing an endorsement.
    /// Arguments:
    /// - endorsement_id: the endorsement id to get the block id of
    ///
    /// Returns:
    /// - the list of block id containing the endorsement
    pub fn get_blocks_by_endorsement(&self, endorsement_id: &EndorsementId) -> Set<BlockId> {
        match self.index_by_endorsement.get(endorsement_id) {
            Some(blocks) => blocks.clone(),
            None => Set::default(),
        }
    }
}
