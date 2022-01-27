use crate::{
    ledger_models::LedgerChanges,
    prehash::{Map, Set},
    rolls::RollUpdates,
    Address, Block, BlockId, EndorsementId, OperationId,
};

/// Block that was checked as final, with some useful precomputed data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActiveBlock {
    /// The creator's address
    pub creator_address: Address,
    /// The block itself, as it was created
    pub block: Block,
    /// one (block id, period) per thread ( if not genesis )
    pub parents: Vec<(BlockId, u64)>,
    /// one HashMap<Block id, period> per thread (blocks that need to be kept)
    /// Children reference that block as a parent
    pub children: Vec<Map<BlockId, u64>>,
    /// dependencies required for validity check
    pub dependencies: Set<BlockId>,
    /// Blocks id that have this block as an ancestor
    pub descendants: Set<BlockId>,
    /// ie has its fitness reached the given threshold
    pub is_final: bool,
    /// Changes caused by this block
    pub block_ledger_changes: LedgerChanges,
    /// index in the block, end of validity period
    pub operation_set: Map<OperationId, (usize, u64)>,
    /// IDs of the endorsements to index in block
    pub endorsement_ids: Map<EndorsementId, u32>,
    /// Maps addresses to operations id they are involved in
    pub addresses_to_operations: Map<Address, Set<OperationId>>,
    /// Maps addresses to endorsements id they are involved in
    pub addresses_to_endorsements: Map<Address, Set<EndorsementId>>,
    /// Address -> RollUpdate
    pub roll_updates: RollUpdates,
    /// list of (period, address, did_create) for all block/endorsement creation events
    pub production_events: Vec<(u64, Address, bool)>,
}

impl ActiveBlock {
    /// Computes the fitness of the block
    pub fn fitness(&self) -> u64 {
        1 + self.block.header.content.endorsements.len() as u64
    }
}
