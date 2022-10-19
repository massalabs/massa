use crate::block_graph_export::BlockGraphExport;
use crate::{bootstrapable_graph::BootstrapableGraph, error::GraphResult};
use massa_models::{
    api::BlockGraphStatus,
    block::{BlockHeader, BlockId},
    clique::Clique,
    slot::Slot,
    stats::ConsensusStats,
    wrapped::Wrapped,
};
use massa_storage::Storage;

/// interface that communicates with the graph worker thread
pub trait GraphController: Send + Sync {
    fn get_block_graph_status(
        &self,
        start_slot: Option<Slot>,
        end_slot: Option<Slot>,
    ) -> GraphResult<BlockGraphExport>;

    fn get_block_statuses(&self, ids: &Vec<BlockId>) -> Vec<BlockGraphStatus>;

    fn get_cliques(&self) -> Vec<Clique>;

    fn get_bootstrap_graph(&self) -> GraphResult<BootstrapableGraph>;

    fn get_stats(&self) -> GraphResult<ConsensusStats>;

    fn get_best_parents(&self) -> Vec<(BlockId, u64)>;

    fn get_blockclique_block_at_slot(&self, slot: Slot) -> Option<BlockId>;

    fn get_latest_blockclique_block_at_slot(&self, slot: Slot) -> BlockId;

    fn register_block(&self, block_id: BlockId, slot: Slot, block_storage: Storage);

    fn register_block_header(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>);

    fn mark_invalid_block(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>);

    /// Returns a boxed clone of self.
    /// Useful to allow cloning `Box<dyn GraphController>`.
    fn clone_box(&self) -> Box<dyn GraphController>;
}

/// Allow cloning `Box<dyn GraphController>`
/// Uses `GraphController::clone_box` internally
impl Clone for Box<dyn GraphController> {
    fn clone(&self) -> Box<dyn GraphController> {
        self.clone_box()
    }
}

/// Graph manager used to stop the graph thread
pub trait GraphManager {
    /// Stop the graph thread
    /// Note that we do not take self by value to consume it
    /// because it is not allowed to move out of Box<dyn GraphManager>
    /// This will improve if the `unsized_fn_params` feature stabilizes enough to be safely usable.
    fn stop(&mut self);
}
