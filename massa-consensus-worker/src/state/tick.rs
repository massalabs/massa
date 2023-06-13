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
        use massa_models::config::constants::{
            DOWNTIME_END_TIMESTAMP, DOWNTIME_END_TIMESTAMP_BOOTSTRAP, DOWNTIME_START_TIMESTAMP,
        };

        let now = massa_time::MassaTime::now().expect("could not get now time");

        // last_start_period should be set to trigger after the DOWNTIME_END_TIMESTAMP
        let start_time = DOWNTIME_START_TIMESTAMP;
        let end_time = if cfg!(feature = "bootstrap_server") {
            DOWNTIME_END_TIMESTAMP_BOOTSTRAP
        } else {
            DOWNTIME_END_TIMESTAMP
        };

        if now >= start_time && now <= end_time {
            let (days, hours, mins, secs) = DOWNTIME_END_TIMESTAMP
                .saturating_sub(now)
                .days_hours_mins_secs()
                .unwrap();

            panic!(
                "We are in downtime! {} days, {} hours, {} minutes, {} seconds remaining to the end of the downtime",
                days, hours, mins, secs,
            );
        }

        for i in 0..self.latest_final_blocks_periods.len() {
            if let Some((_blockid, period)) = self.latest_final_blocks_periods.get(i) {
                self.massa_metrics.set_consensus_period(i, *period);
            }
        }

        self.massa_metrics.set_consensus_state(
            self.active_index.len(),
            self.incoming_index.len(),
            self.discarded_index.len(),
            self.block_statuses.len(),
            self.active_index_without_ops.len(),
        );

        Ok(())
    }
}
