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
    pub fn slot_tick(&mut self, current_slot: Slot) -> Result<(), ConsensusError> {
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

        // Simulate downtime
        // last_start_period should be set after the downtime_end_period (with a delay, to let people bootstrap before the network restarts).
        let downtime_start_period = 225; // Period 225 is 1 hour after genesis
        let downtime_end_period = 450; // Period 450 is 2 hours after genesis
        if current_slot.period >= downtime_start_period
            && current_slot.period <= downtime_end_period
        {
            panic!(
                "We are in downtime! Current period: {}. Downtime periods: {:?}",
                current_slot.period,
                downtime_start_period..=downtime_end_period
            );
        }

        Ok(())
    }
}
