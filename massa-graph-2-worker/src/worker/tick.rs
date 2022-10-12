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

        // signal tick to block graph
        {
            let mut write_shared_state = self.shared_state.write();
            write_shared_state.slot_tick(Some(slot))?;
            write_shared_state.stats_tick()?;
        }

        // take care of block db changes
        self.block_db_changed()?;

        Ok(())
    }
}
