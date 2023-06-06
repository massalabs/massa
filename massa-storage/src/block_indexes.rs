use std::{collections::hash_map, collections::HashMap};

use massa_models::{
    address::Address,
    block::SecureShareBlock,
    block_id::BlockId,
    endorsement::EndorsementId,
    operation::OperationId,
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
};

/// Container for all blocks and different indexes.
/// Note: The structure can evolve and store more indexes.
#[derive(Default)]
pub struct BlockIndexes {
    /// Blocks structure container
    blocks: PreHashMap<BlockId, SecureShareBlock>,
    /// Structure mapping creators with the created blocks
    index_by_creator: PreHashMap<Address, PreHashSet<BlockId>>,
    /// Structure mapping slot with their block id
    index_by_slot: HashMap<Slot, PreHashSet<BlockId>>,
    /// Structure mapping operation id with ids of blocks they are contained in
    index_by_op: PreHashMap<OperationId, PreHashSet<BlockId>>,
    /// Structure mapping endorsement id with ids of blocks they are contained in
    index_by_endorsement: PreHashMap<EndorsementId, PreHashSet<BlockId>>,
}

impl BlockIndexes {
    /// Insert a block and populate the indexes.
    /// Arguments:
    /// - block: the block to insert

    pub(crate) fn insert(&mut self, block: SecureShareBlock) {
        if let Ok(b) = self.blocks.try_insert(block.id, block) {
            #[cfg(feature = "metrics")]
            {
                use massa_metrics::inc_blocks_counter;
                inc_blocks_counter();
            }

            // update creator index
            self.index_by_creator
                .entry(b.content_creator_address)
                .or_default()
                .insert(b.id);

            // update slot index
            self.index_by_slot
                .entry(b.content.header.content.slot)
                .or_default()
                .insert(b.id);

            // update index_by_op
            for op in &b.content.operations {
                self.index_by_op.entry(*op).or_default().insert(b.id);
            }

            // update index_by_endorsement
            for ed in &b.content.header.content.endorsements {
                self.index_by_endorsement
                    .entry(ed.id)
                    .or_default()
                    .insert(b.id);
            }
        }
    }

    /// Remove a block, remove from the indexes and do some clean-up in indexes if necessary.
    /// Arguments:
    /// * `block_id`: the block id to remove
    pub(crate) fn remove(&mut self, block_id: &BlockId) -> Option<SecureShareBlock> {
        if let Some(b) = self.blocks.remove(block_id) {
            // update creator index
            if let hash_map::Entry::Occupied(mut occ) =
                self.index_by_creator.entry(b.content_creator_address)
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

            // update index_by_op
            for op in &b.content.operations {
                if let hash_map::Entry::Occupied(mut occ) = self.index_by_op.entry(*op) {
                    occ.get_mut().remove(&b.id);
                    if occ.get().is_empty() {
                        occ.remove();
                    }
                }
            }

            // update index_by_endorsement
            for ed in &b.content.header.content.endorsements {
                if let hash_map::Entry::Occupied(mut occ) = self.index_by_endorsement.entry(ed.id) {
                    occ.get_mut().remove(&b.id);
                    if occ.get().is_empty() {
                        occ.remove();
                    }
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
    pub fn get(&self, id: &BlockId) -> Option<&SecureShareBlock> {
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
    pub fn get_blocks_created_by(&self, address: &Address) -> Option<&PreHashSet<BlockId>> {
        self.index_by_creator.get(address)
    }

    /// Get the block ids of the blocks at a given slot.
    /// Arguments:
    /// - slot: the slot to get the block id of
    ///
    /// Returns:
    /// - the block ids of the blocks at the slot if any, None otherwise
    pub fn get_blocks_by_slot(&self, slot: &Slot) -> Option<&PreHashSet<BlockId>> {
        self.index_by_slot.get(slot)
    }

    /// Get the block ids of the blocks containing a given operation.
    /// Arguments:
    /// - id: the ID of the operation
    ///
    /// Returns:
    /// - the block ids containing the operation if any, None otherwise
    pub fn get_blocks_by_operation(&self, id: &OperationId) -> Option<&PreHashSet<BlockId>> {
        self.index_by_op.get(id)
    }

    /// Get the block ids of the blocks containing a given endorsement.
    /// Arguments:
    /// - id: the ID of the endorsement
    ///
    /// Returns:
    /// - the block ids containing the endorsement if any, None otherwise
    pub fn get_blocks_by_endorsement(&self, id: &EndorsementId) -> Option<&PreHashSet<BlockId>> {
        self.index_by_endorsement.get(id)
    }
}
