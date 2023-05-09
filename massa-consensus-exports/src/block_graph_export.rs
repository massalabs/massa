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
pub(crate)  struct BlockGraphExport {
    /// Genesis blocks.
    pub(crate)  genesis_blocks: Vec<BlockId>,
    /// Map of active blocks, were blocks are in their exported version.
    pub(crate)  active_blocks: PreHashMap<BlockId, ExportCompiledBlock>,
    /// Finite cache of discarded blocks, in exported version `(slot, creator_address, parents)`.
    pub(crate)  discarded_blocks: PreHashMap<BlockId, (DiscardReason, (Slot, Address, Vec<BlockId>))>,
    /// Best parents hashes in each thread.
    pub(crate)  best_parents: Vec<(BlockId, u64)>,
    /// Latest final period and block hash in each thread.
    pub(crate)  latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// Head of the incompatibility graph.
    pub(crate)  gi_head: PreHashMap<BlockId, PreHashSet<BlockId>>,
    /// List of maximal cliques of compatible blocks.
    pub(crate)  max_cliques: Vec<Clique>,
}
