use massa_models::{
    prehash::{Map, Set},
    Address, BlockId, EndorsementId, WrappedEndorsement,
};

/// Container for all endorsements and different indexes.
/// Note: The structure can evolve and store more indexes.
#[derive(Default)]
pub struct EndorsementIndexes {
    /// Endorsements structure container
    pub(crate) endorsements: Map<EndorsementId, WrappedEndorsement>,
    /// Structure mapping creators with the created endorsements
    index_by_creator: Map<Address, Set<EndorsementId>>,
    /// Structure mapping block ids with the endorsements
    index_by_block: Map<BlockId, Set<EndorsementId>>,
}

impl EndorsementIndexes {
    /// Insert a batch of endorsements and populate the indexes.
    /// Arguments:
    /// - endorsements: the endorsements to insert
    pub(crate) fn batch_insert(&mut self, endorsements: Vec<WrappedEndorsement>) {
        for endorsement in endorsements {
            let id = endorsement.id;
            let creator = endorsement.creator_address;
            self.endorsements.entry(id).or_insert(endorsement);
            self.index_by_creator.entry(creator).or_default().insert(id);
        }
    }

    /// Remove a batch of endorsements, remove from the indexes and made some clean-up in indexes if necessary.
    /// Arguments:
    /// - endorsement_ids: the endorsement ids to remove
    pub(crate) fn batch_remove(&mut self, endorsement_ids: Vec<EndorsementId>) {
        for id in endorsement_ids {
            let endorsement = self
                .endorsements
                .remove(&id)
                .expect("removing absent object from storage");
            let creator = endorsement.creator_address;
            let entry = self.index_by_creator.entry(creator).or_default();
            entry.remove(&id);
            if entry.is_empty() {
                self.index_by_creator.remove(&creator);
            }
        }
    }

    /// Link a vector of endorsements to a block. Should be used in case of the block is added to the storage.
    /// Arguments:
    /// - block_id: the block id to link the endorsements to
    /// - endorsement_ids: the endorsements to link to the block
    pub(crate) fn link_endorsements_with_block(
        &mut self,
        block_id: &BlockId,
        endorsement_ids: &Vec<EndorsementId>,
    ) {
        self.index_by_block
            .entry(*block_id)
            .or_default()
            .extend(endorsement_ids);
    }

    /// Unlink a vector of endorsements from a block. Should be used in case of the block is removed from the storage.
    /// Arguments:
    /// - block_id: the block id to unlink the endorsements from
    pub(crate) fn unlink_endorsements_from_block(&mut self, block_id: &BlockId) {
        self.index_by_block.remove(block_id);
    }

    /// Get endorsements created by an address
    /// Arguments:
    /// - address: the address to get the endorsements created by
    ///
    /// Returns:
    /// - the endorsements created by the address
    pub fn _get_endorsements_created_by(&self, address: &Address) -> Vec<EndorsementId> {
        match self.index_by_creator.get(address) {
            Some(endorsements) => endorsements.iter().cloned().collect(),
            None => Vec::new(),
        }
    }

    /// Get endorsements by the id of the block they are contained in
    /// Arguments:
    /// - block_id: the id of the block to get the endorsements from
    ///
    /// Returns:
    /// - the endorsements contained in the block
    pub fn _get_endorsements_in_block(&self, block_id: &BlockId) -> Vec<EndorsementId> {
        match self.index_by_block.get(block_id) {
            Some(endorsements) => endorsements.iter().cloned().collect(),
            None => Vec::new(),
        }
    }
}
