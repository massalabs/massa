use std::collections::BTreeSet;

use massa_consensus_exports::{
    block_status::{BlockStatus, DiscardReason, HeaderOrBlock},
    error::ConsensusError,
};
use massa_logging::massa_trace;
use massa_models::{block_header::SecuredHeader, block_id::BlockId, slot::Slot};
use massa_storage::Storage;
use massa_time::MassaTime;
use tracing::{debug, log::warn};

use super::ConsensusState;

impl ConsensusState {
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
        header: SecuredHeader,
        current_slot: Option<Slot>,
    ) -> Result<(), ConsensusError> {
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
        match self.blocks_state.insert_block(
            block_id,
            BlockStatus::Incoming(HeaderOrBlock::Header(header.clone())),
        ) {
            Ok(None) => {
                to_ack.insert((header.content.slot, block_id));
            }
            Ok(Some(_)) => {
                let mut to_increase = false;
                let sequence_counter = self.blocks_state.sequence_counter();
                let occ = self.blocks_state.get_mut(&block_id).unwrap();
                match occ {
                    BlockStatus::Discarded {
                        sequence_number, ..
                    } => {
                        // promote if discarded
                        *sequence_number = sequence_counter + 1;
                        to_increase = true;
                    }
                    BlockStatus::WaitingForDependencies { .. } => {
                        // promote in dependencies
                        self.blocks_state.promote_dep_tree(block_id)?;
                    }
                    _ => {}
                }
                if to_increase {
                    self.blocks_state.inc_sequence_counter();
                }
            }
            Err(e) => {
                warn!("couldn't store header {} received: {}", block_id, e);
                return Err(e);
            }
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
    /// * `created`: is the block created by the node or received from the network
    ///
    /// # Returns:
    ///  Success or error if the block is invalid or too old
    pub fn register_block(
        &mut self,
        block_id: BlockId,
        slot: Slot,
        current_slot: Option<Slot>,
        storage: Storage,
        created: bool,
    ) -> Result<(), ConsensusError> {
        // ignore genesis blocks
        if self.genesis_hashes.contains(&block_id) {
            return Ok(());
        }

        // Block is coming from protocol mark it for desync calculation
        if !created {
            let now = MassaTime::now()?;
            self.protocol_blocks.push_back((now, block_id));
        }

        debug!("received block {} for slot {}", block_id, slot);

        let mut to_ack: BTreeSet<(Slot, BlockId)> = BTreeSet::new();
        match self.blocks_state.insert_block(
            block_id,
            BlockStatus::Incoming(HeaderOrBlock::Block {
                id: block_id,
                slot,
                storage: storage.clone(),
            }),
        ) {
            Ok(None) => {
                to_ack.insert((slot, block_id));
            }
            Ok(Some(_)) => {
                let mut to_increase = false;
                let sequence_counter = self.blocks_state.sequence_counter();
                let occ = self.blocks_state.get_mut(&block_id).unwrap();
                match occ {
                    BlockStatus::Discarded {
                        sequence_number, ..
                    } => {
                        // promote if discarded
                        *sequence_number = sequence_counter + 1;
                        to_increase = true;
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
                        self.blocks_state.promote_dep_tree(block_id)?;
                    }
                    _ => {
                        return Ok(());
                    }
                }
                if to_increase {
                    self.blocks_state.inc_sequence_counter();
                }
            }
            Err(e) => {
                warn!("couldn't store block {} received: {}", block_id, e);
                return Err(e);
            }
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
    pub fn mark_invalid_block(
        &mut self,
        block_id: &BlockId,
        header: SecuredHeader,
    ) -> Result<(), ConsensusError> {
        let reason = DiscardReason::Invalid("invalid".to_string());
        self.maybe_note_attack_attempt(&reason, block_id);
        massa_trace!("consensus.block_graph.process.invalid_block", {"block_id": block_id, "reason": reason});
        self.blocks_state.update_block_state(
            block_id,
            BlockStatus::Discarded {
                slot: header.content.slot,
                creator: header.content_creator_address,
                parents: header.content.parents,
                reason,
                sequence_number: self.blocks_state.sequence_counter(),
            },
        )
    }
}
