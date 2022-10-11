use std::cmp::max;

use super::GraphWorker;
use massa_graph::error::GraphResult;
use massa_models::stats::ConsensusStats;
use massa_time::MassaTime;

impl GraphWorker {
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
        let clique_count = self.shared_state.read().get_clique_count() as u64;
        Ok(ConsensusStats {
            final_block_count,
            stale_block_count,
            clique_count,
            start_timespan: timespan_start,
            end_timespan: timespan_end,
        })
    }
}
