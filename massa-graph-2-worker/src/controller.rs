use massa_graph::{error::GraphResult, BlockGraphExport, BootstrapableGraph};
use massa_graph_2_exports::GraphController;
use massa_models::{
    api::BlockGraphStatus,
    block::{BlockHeader, BlockId},
    clique::Clique,
    slot::Slot,
    stats::ConsensusStats,
    wrapped::Wrapped,
};
use massa_storage::Storage;
use std::sync::mpsc::SyncSender;

use crate::commands::GraphCommand;

#[derive(Clone)]
pub struct GraphControllerImpl {
    pub command_sender: SyncSender<GraphCommand>,
}

impl GraphController for GraphControllerImpl {
    fn get_block_graph_status(
        &self,
        start_slot: Option<Slot>,
        end_slot: Option<Slot>,
    ) -> GraphResult<BlockGraphExport> {
        todo!()
    }

    fn get_block_statuses(&self, ids: Vec<BlockId>) -> BlockGraphStatus {
        todo!()
    }

    fn get_cliques(&self) -> Vec<Clique> {
        todo!()
    }

    fn get_bootstrap_graph(&self) -> GraphResult<BootstrapableGraph> {
        todo!()
    }

    fn get_stats(&self) -> GraphResult<ConsensusStats> {
        todo!()
    }

    fn get_best_parents(&self) -> &Vec<(BlockId, u64)> {
        todo!()
    }

    fn get_blockclique_block_at_slot(&self, slot: Slot) -> Option<BlockId> {
        todo!()
    }

    fn get_latest_blockclique_block_at_slot(&self, slot: Slot) -> BlockId {
        todo!()
    }

    fn register_block(
        &self,
        block_id: BlockId,
        slot: Slot,
        block_storage: Storage,
    ) -> GraphResult<()> {
        todo!()
    }

    fn register_block_header(
        &self,
        block_id: BlockId,
        header: Wrapped<BlockHeader, BlockId>,
    ) -> GraphResult<()> {
        todo!()
    }

    fn mark_invalid_block(
        &self,
        block_id: BlockId,
        header: Wrapped<BlockHeader, BlockId>,
    ) -> GraphResult<()> {
        todo!()
    }
}
