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
    pub(crate) fn batch_insert(&mut self, endorsements: Vec<WrappedEndorsement>) {
        for endorsement in endorsements {
            let id = endorsement.id;
            let creator = endorsement.creator_address;
            self.endorsements.entry(id).or_insert(endorsement);
            self.index_by_creator.entry(creator).or_default().insert(id);
        }
    }

    pub(crate) fn batch_remove(&mut self, endorsement_ids: Vec<EndorsementId>) {
        for id in endorsement_ids {
            let operation = self
                .endorsements
                .remove(&id)
                .expect("removing absent object from storage");
            let creator = operation.creator_address;
            self.index_by_creator
                .entry(creator)
                .or_default()
                .remove(&id);
        }
    }

    pub(crate) fn link_endorsements_with_block(
        &mut self,
        block_id: &BlockId,
        operations: &Vec<EndorsementId>,
    ) {
        self.index_by_block
            .entry(*block_id)
            .or_default()
            .extend(operations);
    }

    pub(crate) fn unlink_endorsements_from_block(&mut self, block_id: &BlockId) {
        self.index_by_block.remove(block_id);
    }

    pub fn get_endorsements_created_by(&self, address: &Address) -> Vec<WrappedEndorsement> {
        match self.index_by_creator.get(address) {
            Some(ids) => ids
                .iter()
                .filter_map(|id| self.endorsements.get(id))
                .cloned()
                .collect(),
            _ => return Vec::default(),
        }
    }

    pub fn get_endorsements_in_block(&self, block_id: &BlockId) -> Vec<WrappedEndorsement> {
        match self.index_by_block.get(block_id) {
            Some(ids) => ids
                .iter()
                .filter_map(|id| self.endorsements.get(id))
                .cloned()
                .collect(),
            _ => return Vec::default(),
        }
    }
}
