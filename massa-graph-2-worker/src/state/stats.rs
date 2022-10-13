use super::GraphState;
use massa_graph::error::GraphResult;
use massa_graph_2_exports::events::GraphEvent;
use massa_models::stats::ConsensusStats;
use massa_time::MassaTime;
use std::cmp::max;
use tracing::log::warn;

impl GraphState {
    /// Calculate and return stats about graph
    pub fn get_stats(&self) -> GraphResult<ConsensusStats> {
        let timespan_end = max(
            self.launch_time,
            MassaTime::now(self.config.clock_compensation_millis)?,
        );
        let timespan_start = max(
            timespan_end.saturating_sub(self.config.stats_timespan),
            self.launch_time,
        );
        let final_block_count = self
            .final_block_stats
            .iter()
            .filter(|(t, _, _)| *t >= timespan_start && *t < timespan_end)
            .count() as u64;
        let stale_block_count = self
            .stale_block_stats
            .iter()
            .filter(|t| **t >= timespan_start && **t < timespan_end)
            .count() as u64;
        let clique_count = self.get_clique_count() as u64;
        Ok(ConsensusStats {
            final_block_count,
            stale_block_count,
            clique_count,
            start_timespan: timespan_start,
            end_timespan: timespan_end,
        })
    }

    /// Must be called each tick to update stats. Will detect if a desynchronization happened
    pub fn stats_tick(&mut self) -> GraphResult<()> {
        let now = MassaTime::now(self.config.clock_compensation_millis)?;

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
        // prune stats
        self.prune_stats()?;
        Ok(())
    }

    /// Remove old stats from graph storage
    pub fn prune_stats(&mut self) -> GraphResult<()> {
        let start_time = MassaTime::now(self.config.clock_compensation_millis)?
            .saturating_sub(self.stats_history_timespan);
        while let Some((t, _, _)) = self.final_block_stats.front() {
            if t < &start_time {
                self.final_block_stats.pop_front();
            } else {
                break;
            }
        }
        while let Some(t) = self.stale_block_stats.front() {
            if t < &start_time {
                self.stale_block_stats.pop_front();
            } else {
                break;
            }
        }
        while let Some((t, _)) = self.protocol_blocks.front() {
            if t < &start_time {
                self.protocol_blocks.pop_front();
            } else {
                break;
            }
        }
        Ok(())
    }
}
