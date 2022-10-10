use massa_graph::error::GraphResult;
use massa_time::MassaTime;

use super::GraphWorker;

impl GraphWorker {
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
