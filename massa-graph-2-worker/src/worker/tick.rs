use massa_graph::error::GraphResult;
use massa_graph_2_exports::events::GraphEvent;
use massa_logging::massa_trace;
use massa_models::slot::Slot;
use massa_time::MassaTime;
use std::cmp::max;
use tracing::{info, log::warn};

use super::GraphWorker;

impl GraphWorker {
    pub fn slot_tick(&mut self, slot: Slot) -> GraphResult<()> {
        let now = MassaTime::now(self.config.clock_compensation_millis)?;
        massa_trace!("consensus.consensus_worker.slot_tick", { "slot": slot });

        let previous_cycle = self
            .previous_slot
            .map(|s| s.get_cycle(self.config.periods_per_cycle));
        let observed_cycle = slot.get_cycle(self.config.periods_per_cycle);
        if previous_cycle.is_none() {
            // first cycle observed
            info!("Massa network has started ! 🎉")
        }
        if previous_cycle < Some(observed_cycle) {
            info!("Started cycle {}", observed_cycle);
        }

        // check if there are any final blocks is coming from protocol
        // if none => we are probably desync
        #[cfg(not(feature = "sandbox"))]
        if now
            > max(self.config.genesis_timestamp, self.launch_time)
                .saturating_add(self.stats_desync_detection_timespan)
            && !self
                .final_block_stats
                .iter()
                .any(|(time, _, is_from_protocol)| {
                    time > &now.saturating_sub(self.stats_desync_detection_timespan)
                        && *is_from_protocol
                })
        {
            warn!("desynchronization detected because the recent final block history is empty or contains only blocks produced by this node");
            let _ = self.channels.controller_event_tx.send(GraphEvent::NeedSync);
        }

        // signal tick to block graph
        self.shared_state.write().slot_tick(Some(slot))?;

        // take care of block db changes
        self.block_db_changed()?;

        // prune stats
        self.prune_stats()?;
        Ok(())
    }
}
