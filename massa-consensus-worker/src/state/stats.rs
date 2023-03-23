use super::ConsensusState;
use massa_consensus_exports::error::ConsensusError;
use massa_models::stats::ConsensusStats;
use massa_time::MassaTime;
use std::cmp::max;

/*#[cfg(not(feature = "sandbox"))]
use tracing::log::warn;

#[cfg(not(feature = "sandbox"))]
use massa_consensus_exports::events::ConsensusEvent;
*/

impl ConsensusState {
    /// Calculate and return stats about consensus
    pub fn get_stats(&self) -> Result<ConsensusStats, ConsensusError> {
        let timespan_end = max(self.launch_time, MassaTime::now()?);
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
    pub fn stats_tick(&mut self) -> Result<(), ConsensusError> {
        // check if there are any final blocks is coming from protocol
        // if none => we are probably desync
        // TODO: Reenable desync detection
        /*#[cfg(not(feature = "sandbox"))]
        {
            let now = MassaTime::now()?;
            if now
                > max(
                    self.config
                        .genesis_timestamp
                        .checked_add(self.config.t0.checked_mul(self.config.last_start_period)?)?,
                    self.launch_time,
                )
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
                let _ = self
                    .channels
                    .controller_event_tx
                    .send(ConsensusEvent::NeedSync);
            }
        }*/
        // prune stats
        self.prune_stats()?;
        Ok(())
    }

    /// Remove old stats from consensus storage
    pub fn prune_stats(&mut self) -> Result<(), ConsensusError> {
        let start_time = MassaTime::now()?.saturating_sub(self.stats_history_timespan);
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
