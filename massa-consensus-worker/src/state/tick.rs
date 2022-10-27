use std::collections::BTreeSet;

use massa_consensus_exports::{block_status::BlockStatus, error::ConsensusError};
use massa_logging::massa_trace;
use massa_models::{block::BlockId, slot::Slot};
use tracing::info;

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
        info!("AURELIEN: Size of gi_head {}", self.gi_head.len());
        info!("AURELIEN: Size of cliques {}", self.max_cliques.len());
        info!("AURELIEN: Size of active_index {}", self.active_index.len());
        info!(
            "AURELIEN: Size of save_final_periods {}",
            self.save_final_periods.len()
        );
        info!(
            "AURELIEN: Size of latest_final_blocks_periods {}",
            self.latest_final_blocks_periods.len()
        );
        info!("AURELIEN: Size of best_parents {}", self.best_parents.len());
        info!(
            "AURELIEN: Size of block_statuses {}",
            self.block_statuses.len()
        );
        info!(
            "AURELIEN: Size of incoming_index {}",
            self.incoming_index.len()
        );
        info!(
            "AURELIEN: Size of waiting_for_slot_index {}",
            self.waiting_for_slot_index.len()
        );
        info!(
            "AURELIEN: Size of waiting_for_dependencies_index {}",
            self.waiting_for_dependencies_index.len()
        );
        info!(
            "AURELIEN: Size of discarded_index {}",
            self.discarded_index.len()
        );
        info!("AURELIEN: Size of to_propagate {}", self.to_propagate.len());
        info!(
            "AURELIEN: Size of attack_attempts {}",
            self.attack_attempts.len()
        );
        info!(
            "AURELIEN: Size of new_final_blocks {}",
            self.new_final_blocks.len()
        );
        info!(
            "AURELIEN: Size of new_stale_blocks {}",
            self.new_stale_blocks.len()
        );
        info!(
            "AURELIEN: Size of final_block_stats {}",
            self.final_block_stats.len()
        );
        info!(
            "AURELIEN: Size of protocol_blocks {}",
            self.protocol_blocks.len()
        );
        info!(
            "AURELIEN: Size of stale_block_stats {}",
            self.stale_block_stats.len()
        );
        info!("AURELIEN: Size of wishlist {}", self.wishlist.len());
        info!(
            "AURELIEN: Size of prev_blockclique {}",
            self.prev_blockclique.len()
        );
        info!(
            "AURELIEN: Size of storage block length {}",
            self.storage.get_block_refs().len()
        );
        info!(
            "AURELIEN: Size of storage endos length {}",
            self.storage.get_endorsement_refs().len()
        );

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

        self.stats_tick()?;
        // take care of block db changes
        self.block_db_changed()?;

        Ok(())
    }
}
