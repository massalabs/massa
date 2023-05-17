use std::collections::BTreeSet;

use massa_consensus_exports::{block_status::BlockStatus, error::ConsensusError};
use massa_logging::massa_trace;
use massa_models::{block_id::BlockId, slot::Slot};

use super::ConsensusState;

impl ConsensusState {
    /// This function should be called each tick and will check if there is a block in the graph that should be processed at this slot, and if so, process it.
    ///
    /// # Arguments:
    /// * `current_slot`: the current slot
    ///
    /// # Returns:
    /// Error if the process of a block returned an error.
    pub(crate) fn slot_tick(&mut self, current_slot: Slot) -> Result<(), ConsensusError> {
        massa_trace!("consensus.consensus_worker.slot_tick", {
            "slot": current_slot
        });

        // list all elements for which the time has come
        let to_process: BTreeSet<(Slot, BlockId)> = self
            .waiting_for_slot_index
            .iter()
            .filter_map(|b_id| match self.block_statuses.get(b_id) {
                Some(BlockStatus::WaitingForSlot(header_or_block)) => {
                    let slot = header_or_block.get_slot();
                    if slot <= current_slot {
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
        self.rec_process(to_process, Some(current_slot))?;

        // Update the stats
        self.stats_tick()?;

        // take care of block db changes
        self.block_db_changed()?;

        Ok(())
    }
}
