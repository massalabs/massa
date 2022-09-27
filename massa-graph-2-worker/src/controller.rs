use massa_graph::{error::GraphResult, BlockGraphExport, BootstrapableGraph};
use massa_graph_2_exports::{GraphController, GraphState};
use massa_models::{
    api::BlockGraphStatus,
    block::{BlockHeader, BlockId},
    clique::Clique,
    slot::Slot,
    stats::ConsensusStats,
    wrapped::Wrapped,
};
use massa_storage::Storage;
use parking_lot::RwLock;
use std::sync::{mpsc::SyncSender, Arc};

use crate::commands::GraphCommand;

#[derive(Clone)]
pub struct GraphControllerImpl {
    command_sender: SyncSender<GraphCommand>,
    shared_state: Arc<RwLock<GraphState>>,
}

impl GraphControllerImpl {
    pub fn new(
        command_sender: SyncSender<GraphCommand>,
        shared_state: Arc<RwLock<GraphState>>,
    ) -> Self {
        Self {
            command_sender,
            shared_state,
        }
    }
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

    fn register_block(&self, block_id: BlockId, slot: Slot, block_storage: Storage) {
        todo!()
    }

    fn register_block_header(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>) {
        todo!()
    }

    fn mark_invalid_block(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>) {
        todo!()
    }
}
