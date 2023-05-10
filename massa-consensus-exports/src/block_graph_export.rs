use massa_models::{
    address::Address,
    block_id::BlockId,
    clique::Clique,
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
};

use crate::block_status::{DiscardReason, ExportCompiledBlock};

/// Bootstrap compatible version of the block graph
#[derive(Debug, Clone)]
#[allow(clippy::type_complexity)]
pub struct BlockGraphExport {
    /// Genesis blocks.
    pub genesis_blocks: Vec<BlockId>,
    /// Map of active blocks, were blocks are in their exported version.
    pub active_blocks: PreHashMap<BlockId, ExportCompiledBlock>,
    /// Finite cache of discarded blocks, in exported version `(slot, creator_address, parents)`.
    pub discarded_blocks: PreHashMap<BlockId, (DiscardReason, (Slot, Address, Vec<BlockId>))>,
    /// Best parents hashes in each thread.
    pub best_parents: Vec<(BlockId, u64)>,
    /// Latest final period and block hash in each thread.
    pub latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// Head of the incompatibility graph.
    pub gi_head: PreHashMap<BlockId, PreHashSet<BlockId>>,
    /// List of maximal cliques of compatible blocks.
    pub max_cliques: Vec<Clique>,
}
