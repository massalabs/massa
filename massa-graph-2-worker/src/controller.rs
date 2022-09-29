use massa_graph::{
    error::{GraphError, GraphResult},
    export_active_block::ExportActiveBlock,
    BootstrapableGraph,
};
use massa_graph_2_exports::{
    block_graph_export::BlockGraphExport, block_status::BlockStatus, GraphController,
};
use massa_models::{
    api::BlockGraphStatus,
    block::{BlockHeader, BlockId},
    clique::Clique,
    prehash::PreHashSet,
    slot::Slot,
    stats::ConsensusStats,
    wrapped::Wrapped,
};
use massa_storage::Storage;
use parking_lot::RwLock;
use std::sync::{mpsc::SyncSender, Arc};

use crate::{commands::GraphCommand, state::GraphState};

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
        self.shared_state
            .read()
            .extract_block_graph_part(start_slot, end_slot)
    }

    fn get_block_statuses(&self, ids: Vec<BlockId>) -> Vec<BlockGraphStatus> {
        let read_shared_state = self.shared_state.read();
        ids.iter()
            .map(|id| read_shared_state.get_block_status(id))
            .collect()
    }

    fn get_cliques(&self) -> Vec<Clique> {
        self.shared_state.read().max_cliques.clone()
    }

    fn get_bootstrap_graph(&self) -> GraphResult<BootstrapableGraph> {
        let read_shared_state = self.shared_state.read();
        let mut required_final_blocks: PreHashSet<_> =
            read_shared_state.list_required_active_blocks()?;
        required_final_blocks.retain(|b_id| {
            if let Some(BlockStatus::Active { a_block, .. }) =
                read_shared_state.block_statuses.get(b_id)
            {
                if a_block.is_final {
                    // filter only final actives
                    return true;
                }
            }
            false
        });
        let mut final_blocks: Vec<ExportActiveBlock> =
            Vec::with_capacity(required_final_blocks.len());
        for b_id in &required_final_blocks {
            if let Some(BlockStatus::Active { a_block, storage }) =
                read_shared_state.block_statuses.get(b_id)
            {
                final_blocks.push(ExportActiveBlock::from_active_block(a_block, storage));
            } else {
                return Err(GraphError::ContainerInconsistency(format!(
                    "block {} was expected to be active but wasn't on bootstrap graph export",
                    b_id
                )));
            }
        }

        Ok(BootstrapableGraph { final_blocks })
    }

    fn get_stats(&self) -> GraphResult<ConsensusStats> {
        todo!()
    }

    fn get_best_parents(&self) -> Vec<(BlockId, u64)> {
        self.shared_state.read().best_parents.clone()
    }

    fn get_blockclique_block_at_slot(&self, slot: Slot) -> Option<BlockId> {
        self.shared_state
            .read()
            .get_blockclique_block_at_slot(&slot)
    }

    fn get_latest_blockclique_block_at_slot(&self, slot: Slot) -> BlockId {
        self.shared_state
            .read()
            .get_latest_blockclique_block_at_slot(&slot)
    }

    fn register_block(&self, block_id: BlockId, slot: Slot, block_storage: Storage) {
        let _ = self.command_sender.try_send(GraphCommand::RegisterBlock(
            block_id,
            slot,
            block_storage,
        ));
    }

    fn register_block_header(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>) {
        let _ = self
            .command_sender
            .try_send(GraphCommand::RegisterBlockHeader(block_id, header));
    }

    fn mark_invalid_block(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>) {
        let _ = self
            .command_sender
            .try_send(GraphCommand::MarkInvalidBlock(block_id, header));
    }
}
