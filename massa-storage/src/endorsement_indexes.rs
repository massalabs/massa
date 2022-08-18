use std::collections::hash_map;

use massa_models::{
    prehash::{Map, Set},
    Address, BlockId, EndorsementId, WrappedEndorsement,
};

/// Container for all endorsements and different indexes.
/// Note: The structure can evolve and store more indexes.
#[derive(Default)]
pub struct EndorsementIndexes {
    /// Endorsements structure container
    endorsements: Map<EndorsementId, WrappedEndorsement>,
    /// Structure mapping creators with the created endorsements
    index_by_creator: Map<Address, Set<EndorsementId>>,
    /// Structure mapping endorsement IDs to their containing blocks
    index_to_block: Map<EndorsementId, Set<BlockId>>,
}

impl EndorsementIndexes {
    /// Insert an endorsement and populate the indexes.
    /// Arguments:
    /// - endorsement: the endorsement to insert
    pub(crate) fn insert(&mut self, endorsement: WrappedEndorsement) {
        if let Ok(e) = self.endorsements.try_insert(endorsement.id, endorsement) {
            // update creator index
            self.index_by_creator
                .entry(e.creator_address)
                .or_default()
                .insert(e.id);
        }
    }

    /// Link a vector of endorsements to a block. Should be used in case of the block is added to the storage.
    /// Arguments:
    /// - block_id: the block id to link the endorsements to
    /// - endorsement_ids: the endorsements to link to the block
    pub(crate) fn link_block(&mut self, block_id: &BlockId, endorsement_ids: &Vec<EndorsementId>) {
        for endorsement_id in endorsement_ids {
            // update to-block index
            self.index_to_block
                .entry(*endorsement_id)
                .or_default()
                .insert(*block_id);
        }
    }

    /// Unlinks a block from the endorsements. Should be used when a block is removed from storage.
    /// Arguments:
    /// - block_id: the block id to unlink the endorsements from
    /// - endorsement_ids: the endorsements to unlink from the block
    pub(crate) fn unlink_block(
        &mut self,
        block_id: &BlockId,
        endorsement_ids: &Vec<EndorsementId>,
    ) {
        for endorsement_id in endorsement_ids {
            if let hash_map::Entry::Occupied(mut occ) = self.index_to_block.entry(*endorsement_id) {
                occ.get_mut().remove(block_id);
                if occ.get().is_empty() {
                    occ.remove();
                }
            }
        }
    }

    /// Remove a endorsement, remove from the indexes and made some clean-up in indexes if necessary.
    /// Arguments:
    /// - endorsement_id: the endorsement id to remove
    pub(crate) fn remove(&mut self, endorsement_id: &EndorsementId) -> Option<WrappedEndorsement> {
        // update to-block index
        self.index_to_block.remove(endorsement_id);

        if let Some(e) = self.endorsements.remove(endorsement_id) {
            // update creator index
            self.index_by_creator
                .entry(e.creator_address)
                .and_modify(|s| {
                    s.remove(&e.id);
                });

            return Some(e);
        }

        None
    }

    /// Gets a reference to a stored endorsement, if any.
    pub fn get(&self, id: &EndorsementId) -> Option<&WrappedEndorsement> {
        self.endorsements.get(id)
    }

    /// Checks whether an endorsement exists in global storage.
    pub fn contains(&self, id: &EndorsementId) -> bool {
        self.endorsements.contains_key(id)
    }

    /// Get endorsements created by an address
    /// Arguments:
    /// - address: the address to get the endorsements created by
    ///
    /// Returns:
    /// - optional reference to a set of endorsements created by that address
    pub fn get_endorsements_created_by(&self, address: &Address) -> Option<&Set<EndorsementId>> {
        self.index_by_creator.get(address)
    }

    /// Get the blocks that contain a given endorsement
    /// Arguments:
    /// - endorsement_id: the id of the endorsement
    ///
    /// Returns:
    /// - an optional reference to a set of block IDs containing the endorsement
    pub fn get_containing_blocks(&self, endorsement_id: &EndorsementId) -> Option<&Set<BlockId>> {
        self.index_to_block.get(endorsement_id)
    }
}
