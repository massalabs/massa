use std::collections::{hash_map, BTreeSet};

use super::GraphWorker;
use massa_graph::error::GraphResult;
use massa_graph_2_exports::block_status::{BlockStatus, HeaderOrBlock};
use massa_logging::massa_trace;
use massa_models::{
    block::{BlockId, WrappedHeader},
    slot::Slot,
};
use tracing::log::debug;

impl GraphWorker {
    pub fn register_block_header(
        &mut self,
        block_id: BlockId,
        header: WrappedHeader,
        current_slot: Option<Slot>,
    ) -> GraphResult<()> {
        let mut write_shared_state = self.shared_state.write();
        // ignore genesis blocks
        if write_shared_state.genesis_hashes.contains(&block_id) {
            return Ok(());
        }

        debug!(
            "received header {} for slot {}",
            block_id, header.content.slot
        );
        massa_trace!("consensus.block_graph.incoming_header", {"block_id": block_id, "header": header});
        let mut to_ack: BTreeSet<(Slot, BlockId)> = BTreeSet::new();
        match write_shared_state.block_statuses.entry(block_id) {
            // if absent => add as Incoming, call rec_ack on it
            hash_map::Entry::Vacant(vac) => {
                to_ack.insert((header.content.slot, block_id));
                vac.insert(BlockStatus::Incoming(HeaderOrBlock::Header(header)));
                write_shared_state.incoming_index.insert(block_id);
            }
            hash_map::Entry::Occupied(mut occ) => match occ.get_mut() {
                BlockStatus::Discarded {
                    sequence_number, ..
                } => {
                    // promote if discarded
                    write_shared_state.new_sequence_number();
                }
                BlockStatus::WaitingForDependencies { .. } => {
                    // promote in dependencies
                    write_shared_state.promote_dep_tree(block_id)?;
                }
                _ => {}
            },
        }

        // process
        write_shared_state.rec_process(to_ack, current_slot)?;

        Ok(())
    }
}
