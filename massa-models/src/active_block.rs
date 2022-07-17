use crate::{
    prehash::{Map, Set},
    Address, BlockId, EndorsementId, OperationId, Slot,
};

/// Block that was checked as valid, with some useful pre-computed data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActiveBlock {
    /// The creator's address
    pub creator_address: Address,
    /// The id of the block
    pub block_id: BlockId,
    /// one (block id, period) per thread ( if not genesis )
    pub parents: Vec<(BlockId, u64)>,
    /// one `HashMap<Block id, period>` per thread (blocks that need to be kept)
    /// Children reference that block as a parent
    pub children: Vec<Map<BlockId, u64>>,
    /// dependencies required for validity check
    pub dependencies: Set<BlockId>,
    /// Blocks id that have this block as an ancestor
    pub descendants: Set<BlockId>,
    /// for example has its fitness reached the given threshold
    pub is_final: bool,
    /// index in the block, end of validity period
    pub operation_set: Map<OperationId, (usize, u64)>,
    /// IDs of the endorsements to index in block
    pub endorsement_ids: Map<EndorsementId, u32>,
    /// Maps addresses to operations id they are involved in
    pub addresses_to_operations: Map<Address, Set<OperationId>>,
    /// Maps addresses to endorsements id they are involved in
    pub addresses_to_endorsements: Map<Address, Set<EndorsementId>>,
    /// Slot of the block.
    pub slot: Slot,
}

impl ActiveBlock {
    /// Computes the fitness of the block
    pub fn fitness(&self) -> u64 {
        1 + self.endorsement_ids.len() as u64
    }
}
