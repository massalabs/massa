use std::collections::BTreeSet;

use massa_graph::error::GraphResult;
use massa_graph_2_exports::block_status::BlockStatus;
use massa_logging::massa_trace;
use massa_models::{block::BlockId, slot::Slot};

use super::GraphState;

impl GraphState {
    pub fn slot_tick(&mut self, actual_slot: Slot) -> GraphResult<()> {
        massa_trace!("consensus.consensus_worker.slot_tick", {
            "slot": actual_slot
        });

        // list all elements for which the time has come
        let to_process: BTreeSet<(Slot, BlockId)> = self
            .waiting_for_slot_index
            .iter()
            .filter_map(|b_id| match self.block_statuses.get(b_id) {
                Some(BlockStatus::WaitingForSlot(header_or_block)) => {
                    let slot = header_or_block.get_slot();
                    if slot <= actual_slot {
                        Some((slot, *b_id))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        massa_trace!("consensus.block_graph.slot_tick", {});
        // process those elements
        self.rec_process(to_process, Some(actual_slot))?;

        self.stats_tick()?;
        // take care of block db changes
        self.block_db_changed()?;

        Ok(())
    }
}
