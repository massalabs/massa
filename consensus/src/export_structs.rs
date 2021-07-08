use models::BlockHeader;

#[derive(Debug, Clone)]
pub enum BlockStatusExport {
    Active,
    Final,
    Discarded,
}

/*
    settlement indicator:

        * delta_f0.saturating_sub(blockclique_fitness - max fitness of the cliques it's not in) => 0 = almost stale, delta_f0 = almost in all cliques
        * delta_f0 - delta_f0.saturating_sub(max fitness of the cliques it it's in) => 0 =  cliques are too shallow ; delta_f0 = depth almost availavle
        * delta_f0 - remaining_fitness => 0 = not final yet; delta_f0 => almost final

*/

#[derive(Debug, Clone)]
pub struct BlockHeaderExport {
    header: BlockHeader,
    status: BlockStatusExport,
}

#[derive(Debug, Clone)]
pub struct BlockGraphExport {
    /// Map of active blocks, were blocks are in their exported version.
    pub active_blocks: HashMap<Hash, ExportCompiledBlock>,
    /// Finite cache of discarded blocks, in exported version.
    pub discarded_blocks: ExportDiscardedBlocks,
    /// Best parents hashe in each thread.
    pub best_parents: Vec<Hash>,
    /// Latest final period and block hash in each thread.
    pub latest_final_blocks_periods: Vec<(Hash, u64)>,
    /// Head of the incompatibility graph.
    pub gi_head: HashMap<Hash, HashSet<Hash>>,
    /// List of maximal cliques of compatible blocks.
    pub max_cliques: Vec<HashSet<Hash>>,
}
