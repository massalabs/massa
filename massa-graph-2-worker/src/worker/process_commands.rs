use std::cmp::max;
use std::collections::{hash_map, BTreeSet};

use super::GraphWorker;
use massa_graph::error::GraphResult;
use massa_graph_2_exports::block_status::{BlockStatus, HeaderOrBlock};
use massa_logging::massa_trace;
use massa_models::{
    block::{BlockId, WrappedHeader},
    slot::Slot,
    stats::ConsensusStats,
};
use massa_time::MassaTime;
use tracing::log::debug;

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

    pub fn register_block_header(
        &mut self,
        block_id: BlockId,
        header: WrappedHeader,
        current_slot: Option<Slot>,
    ) -> GraphResult<()> {
        let mut write_shared_state = self.shared_state.write();
        // ignore genesis blocks
        if write_shared_state.genesis_hashes.contains(&block_id) {
            return Ok(());
        }

        debug!(
            "received header {} for slot {}",
            block_id, header.content.slot
        );
        massa_trace!("consensus.block_graph.incoming_header", {"block_id": block_id, "header": header});
        let mut to_ack: BTreeSet<(Slot, BlockId)> = BTreeSet::new();
        match write_shared_state.block_statuses.entry(block_id) {
            // if absent => add as Incoming, call rec_ack on it
            hash_map::Entry::Vacant(vac) => {
                to_ack.insert((header.content.slot, block_id));
                vac.insert(BlockStatus::Incoming(HeaderOrBlock::Header(header)));
                write_shared_state.incoming_index.insert(block_id);
            }
            hash_map::Entry::Occupied(mut occ) => match occ.get_mut() {
                BlockStatus::Discarded {
                    sequence_number, ..
                } => {
                    // promote if discarded
                    //TODO: Readd this
                    //*sequence_number = write_shared_state.new_sequence_number();
                }
                BlockStatus::WaitingForDependencies { .. } => {
                    // promote in dependencies
                    write_shared_state.promote_dep_tree(block_id)?;
                }
                _ => {}
            },
        }

        // process
        write_shared_state.rec_process(to_ack, current_slot)?;

        Ok(())
    }
}
