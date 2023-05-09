use crate::block_graph_export::BlockGraphExport;
use crate::{bootstrapable_graph::BootstrapableGraph, error::ConsensusError};
use massa_models::prehash::PreHashSet;
use massa_models::streaming_step::StreamingStep;
use massa_models::{
    block::BlockGraphStatus, block_header::BlockHeader, block_id::BlockId, clique::Clique,
    secure_share::SecureShare, slot::Slot, stats::ConsensusStats,
};
use massa_storage::Storage;

/// Interface that communicates with the graph worker thread
pub(crate)  trait ConsensusController: Send + Sync {
    /// Get an export of a part of the graph
    ///
    /// # Arguments
    /// * `start_slot`: the slot to start the export from, if None, the export starts from the genesis
    /// * `end_slot`: the slot to end the export at, if None, the export ends at the current slot
    ///
    /// # Returns
    /// The export of the graph
    fn get_block_graph_status(
        &self,
        start_slot: Option<Slot>,
        end_slot: Option<Slot>,
    ) -> Result<BlockGraphExport, ConsensusError>;

    /// Get statuses of a list of blocks
    ///
    /// # Arguments
    /// * `ids`: the list of block ids to get the status of
    ///
    /// # Returns
    /// The statuses of the blocks sorted by the order of the input list
    fn get_block_statuses(&self, ids: &[BlockId]) -> Vec<BlockGraphStatus>;

    /// Get all the cliques of the graph
    ///
    /// # Returns
    /// The list of cliques
    fn get_cliques(&self) -> Vec<Clique>;

    /// Get a part of the graph to send to a node for it to setup its graph.
    /// Used for bootstrap.
    ///
    /// # Arguments:
    /// * `cursor`: streaming cursor containing the current state of bootstrap and what blocks have previously been sent to the client
    /// * `execution_cursor`: streaming cursor of the final state to ensure that last slot of the bootstrap info match the slot of the execution
    ///
    /// # Returns:
    /// * A portion of the graph
    /// * The list of outdated block ids
    /// * The streaming step value after the current iteration to be saved to be able to use it as parameters and resume the bootstrap
    #[allow(clippy::type_complexity)]
    fn get_bootstrap_part(
        &self,
        cursor: StreamingStep<PreHashSet<BlockId>>,
        execution_cursor: StreamingStep<Slot>,
    ) -> Result<
        (
            BootstrapableGraph,
            PreHashSet<BlockId>,
            StreamingStep<PreHashSet<BlockId>>,
        ),
        ConsensusError,
    >;

    /// Get the stats of the consensus
    ///
    /// # Returns
    /// The stats of the consensus
    fn get_stats(&self) -> Result<ConsensusStats, ConsensusError>;

    /// Get the best parents for the next block to be produced
    ///
    /// # Returns
    /// The id of best parents for the next block to be produced along with their period
    fn get_best_parents(&self) -> Vec<(BlockId, u64)>;

    /// Get the block id of the block at a specific slot in the blockclique
    ///
    /// # Arguments
    /// * `slot`: the slot to get the block id of
    ///
    /// # Returns
    /// The block id of the block at the specified slot if exists
    fn get_blockclique_block_at_slot(&self, slot: Slot) -> Option<BlockId>;

    /// Get the latest block, that is in the blockclique, in the thread of the given slot and before this `slot`.
    ///
    /// # Arguments:
    /// * `slot`: the slot that will give us the thread and the upper bound
    ///
    /// # Returns:
    /// The block id of the latest block in the thread of the given slot and before this slot
    fn get_latest_blockclique_block_at_slot(&self, slot: Slot) -> BlockId;

    /// Register a block in the graph
    ///
    /// # Arguments
    /// * `block_id`: the id of the block to register
    /// * `slot`: the slot of the block
    /// * `block_storage`: the storage that contains all the objects of the block
    /// * `created`: is the block created by our node ?
    fn register_block(&self, block_id: BlockId, slot: Slot, block_storage: Storage, created: bool);

    /// Register a block header in the graph
    ///
    /// # Arguments
    /// * `block_id`: the id of the block to register
    /// * `header`: the header of the block to register
    fn register_block_header(&self, block_id: BlockId, header: SecureShare<BlockHeader, BlockId>);

    /// Mark a block as invalid in the graph
    ///
    /// # Arguments
    /// * `block_id`: the id of the block to mark as invalid
    /// * `header`: the header of the block to mark as invalid
    fn mark_invalid_block(&self, block_id: BlockId, header: SecureShare<BlockHeader, BlockId>);

    /// Returns a boxed clone of self.
    /// Useful to allow cloning `Box<dyn ConsensusController>`.
    fn clone_box(&self) -> Box<dyn ConsensusController>;
}

/// Allow cloning `Box<dyn ConsensusController>`
/// Uses `ConsensusController::clone_box` internally
impl Clone for Box<dyn ConsensusController> {
    fn clone(&self) -> Box<dyn ConsensusController> {
        self.clone_box()
    }
}

/// Consensus manager used to stop the consensus thread
pub(crate)  trait ConsensusManager {
    /// Stop the consensus thread
    /// Note that we do not take self by value to consume it
    /// because it is not allowed to move out of `Box<dyn ConsensusManager>`
    /// This will improve if the `unsized_fn_params` feature stabilizes enough to be safely usable.
    fn stop(&mut self);
}
