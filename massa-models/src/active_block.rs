use crate::{
    prehash::{Map, Set},
    Address, BlockId, Slot,
};

use serde::{Deserialize, Serialize};

/// Block that was checked as valid, with some useful pre-computed data
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Blocks id that have this block as an ancestor
    pub descendants: Set<BlockId>,
    /// for example has its fitness reached the given threshold
    pub is_final: bool,
    /// Slot of the block.
    pub slot: Slot,
    /// Fitness
    pub fitness: u64,
}
