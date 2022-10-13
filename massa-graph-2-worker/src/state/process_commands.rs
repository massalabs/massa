use std::collections::{hash_map::Entry, BTreeSet};

use massa_graph::error::GraphResult;
use massa_graph_2_exports::block_status::{BlockStatus, DiscardReason, HeaderOrBlock};
use massa_logging::massa_trace;
use massa_models::{
    block::{BlockId, WrappedHeader},
    slot::Slot,
};
use massa_storage::Storage;
use tracing::debug;

use super::GraphState;

impl GraphState {
    /// Register a block header in the graph. Ignore genesis hashes.
    ///
    /// # Arguments:
    /// * `block_id`: the block id
    /// * `header`: the header to register
    /// * `current_slot`: the slot when this function is called
    ///
    /// # Returns:
    /// Success or error if the header is invalid or too old
    pub fn register_block_header(
        &mut self,
        block_id: BlockId,
        header: WrappedHeader,
        current_slot: Option<Slot>,
    ) -> GraphResult<()> {
        // ignore genesis blocks
        if self.genesis_hashes.contains(&block_id) {
            return Ok(());
        }

        debug!(
            "received header {} for slot {}",
            block_id, header.content.slot
        );
        massa_trace!("consensus.block_graph.incoming_header", {"block_id": block_id, "header": header});
        let mut to_ack: BTreeSet<(Slot, BlockId)> = BTreeSet::new();
        match self.block_statuses.entry(block_id) {
            // if absent => add as Incoming, call rec_ack on it
            Entry::Vacant(vac) => {
                to_ack.insert((header.content.slot, block_id));
                vac.insert(BlockStatus::Incoming(HeaderOrBlock::Header(header)));
                self.incoming_index.insert(block_id);
            }
            Entry::Occupied(mut occ) => match occ.get_mut() {
                BlockStatus::Discarded {
                    sequence_number, ..
                } => {
                    // promote if discarded
                    self.sequence_counter += 1;
                    *sequence_number = self.sequence_counter;
                }
                BlockStatus::WaitingForDependencies { .. } => {
                    // promote in dependencies
                    self.promote_dep_tree(block_id)?;
                }
                _ => {}
            },
        }

        // process
        self.rec_process(to_ack, current_slot)?;

        Ok(())
    }

    /// Register a new full block in the graph. Ignore genesis hashes.
    ///
    /// # Arguments:
    /// * `block_id`: the block id
    /// * `slot`: the slot of the block
    /// * `current_slot`: the slot when this function is called
    /// * `storage`: Storage containing the whole content of the block
    ///
    /// # Returns:
    ///  Success or error if the block is invalid or too old
    pub fn register_block(
        &mut self,
        block_id: BlockId,
        slot: Slot,
        current_slot: Option<Slot>,
        storage: Storage,
    ) -> GraphResult<()> {
        // ignore genesis blocks
        if self.genesis_hashes.contains(&block_id) {
            return Ok(());
        }

        debug!("received block {} for slot {}", block_id, slot);

        let mut to_ack: BTreeSet<(Slot, BlockId)> = BTreeSet::new();
        match self.block_statuses.entry(block_id) {
            // if absent => add as Incoming, call rec_ack on it
            Entry::Vacant(vac) => {
                to_ack.insert((slot, block_id));
                vac.insert(BlockStatus::Incoming(HeaderOrBlock::Block {
                    id: block_id,
                    slot,
                    storage,
                }));
                self.incoming_index.insert(block_id);
            }
            Entry::Occupied(mut occ) => match occ.get_mut() {
                BlockStatus::Discarded {
                    sequence_number, ..
                } => {
                    // promote if discarded
                    self.sequence_counter += 1;
                    *sequence_number = self.sequence_counter;
                }
                BlockStatus::WaitingForSlot(header_or_block) => {
                    // promote to full block
                    *header_or_block = HeaderOrBlock::Block {
                        id: block_id,
                        slot,
                        storage,
                    };
                }
                BlockStatus::WaitingForDependencies {
                    header_or_block,
                    unsatisfied_dependencies,
                    ..
                } => {
                    // promote to full block and satisfy self-dependency
                    if unsatisfied_dependencies.remove(&block_id) {
                        // a dependency was satisfied: process
                        to_ack.insert((slot, block_id));
                    }
                    *header_or_block = HeaderOrBlock::Block {
                        id: block_id,
                        slot,
                        storage,
                    };
                    // promote in dependencies
                    self.promote_dep_tree(block_id)?;
                }
                _ => return Ok(()),
            },
        }

        // process
        self.rec_process(to_ack, current_slot)?;

        Ok(())
    }

    /// Mark a block that is in the graph as invalid.
    ///
    /// # Arguments:
    /// * `block_id`: Block id of the block to mark as invalid
    /// * `header`: Header of the block to mark as invalid
    pub fn mark_invalid_block(&mut self, block_id: &BlockId, header: WrappedHeader) {
        let reason = DiscardReason::Invalid("invalid".to_string());
        self.maybe_note_attack_attempt(&reason, block_id);
        massa_trace!("consensus.block_graph.process.invalid_block", {"block_id": block_id, "reason": reason});

        // add to discard
        self.block_statuses.insert(
            *block_id,
            BlockStatus::Discarded {
                slot: header.content.slot,
                creator: header.creator_address,
                parents: header.content.parents,
                reason,
                sequence_number: {
                    self.sequence_counter += 1;
                    self.sequence_counter
                },
            },
        );
        self.discarded_index.insert(*block_id);
    }
}
