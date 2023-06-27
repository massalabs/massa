use std::collections::hash_map::Entry;

use massa_consensus_exports::{
    block_status::{BlockStatus, HeaderOrBlock},
    error::ConsensusError,
};
use massa_models::{
    block_id::BlockId,
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
};
use tracing::warn;

pub struct BlocksState {
    /// Every block we know about
    block_statuses: PreHashMap<BlockId, BlockStatus>,
    /// Ids of incoming blocks/headers
    incoming_index: PreHashSet<BlockId>,
    /// Used to limit the number of waiting and discarded blocks
    pub sequence_counter: u64,
    /// ids of waiting for slot blocks/headers
    waiting_for_slot_index: PreHashSet<BlockId>,
    /// ids of waiting for dependencies blocks/headers
    waiting_for_dependencies_index: PreHashSet<BlockId>,
    /// ids of discarded blocks
    discarded_index: PreHashSet<BlockId>,
    /// ids of active blocks
    active_index: PreHashSet<BlockId>,
}

impl BlocksState {
    pub fn new() -> Self {
        Self {
            block_statuses: PreHashMap::default(),
            incoming_index: PreHashSet::default(),
            sequence_counter: 0,
            waiting_for_slot_index: PreHashSet::default(),
            waiting_for_dependencies_index: PreHashSet::default(),
            discarded_index: PreHashSet::default(),
            active_index: PreHashSet::default(),
        }
    }

    pub fn block_statuses(&self) -> &PreHashMap<BlockId, BlockStatus> {
        &self.block_statuses
    }

    pub fn incoming_blocks(&self) -> &PreHashSet<BlockId> {
        &self.incoming_index
    }

    pub fn waiting_for_slot_blocks(&self) -> &PreHashSet<BlockId> {
        &self.waiting_for_slot_index
    }

    pub fn waiting_for_dependencies_blocks(&self) -> &PreHashSet<BlockId> {
        &self.waiting_for_dependencies_index
    }

    pub fn discarded_blocks(&self) -> &PreHashSet<BlockId> {
        &self.discarded_index
    }

    // Internal function to update the indexes
    fn update_indexes(
        &mut self,
        block_id: &BlockId,
        old_block_status: &Option<BlockStatus>,
        new_block_status: &BlockStatus,
    ) {
        if let Some(old_block_status) = old_block_status {
            match old_block_status {
                BlockStatus::Incoming(_) => {
                    self.incoming_index.remove(block_id);
                }
                BlockStatus::WaitingForSlot { .. } => {
                    self.waiting_for_slot_index.remove(block_id);
                }
                BlockStatus::WaitingForDependencies { .. } => {
                    self.waiting_for_dependencies_index.remove(block_id);
                }
                BlockStatus::Discarded { .. } => {
                    self.discarded_index.remove(block_id);
                }
                BlockStatus::Active { .. } => {
                    self.discarded_index.remove(block_id);
                }
            }
        }
        match new_block_status {
            BlockStatus::Incoming(_) => {
                self.incoming_index.insert(block_id.clone());
            }
            BlockStatus::WaitingForSlot { .. } => {
                self.waiting_for_slot_index.insert(block_id.clone());
            }
            BlockStatus::WaitingForDependencies { .. } => {
                self.waiting_for_dependencies_index.insert(block_id.clone());
            }
            BlockStatus::Discarded { .. } => {
                self.discarded_index.insert(block_id.clone());
            }
            BlockStatus::Active { .. } => {
                self.active_index.insert(block_id.clone());
            }
        }
    }

    /// - If the set did not previously contain this value, true is returned.
    /// - If the set already contained this value, false is returned.
    pub fn insert_new_block(
        &mut self,
        block_id: BlockId,
        block_status: BlockStatus,
    ) -> Result<bool, ConsensusError> {
        match self.block_statuses.entry(block_id) {
            Entry::Vacant(vac) => {
                //TODO: Readd in return of this function
                //to_ack.insert((header.content.slot, block_id));
                vac.insert(block_status.clone());
                self.update_indexes(&block_id, &None, &block_status);
                Ok(true)
            }
            Entry::Occupied(mut occ) => {
                match occ.get_mut() {
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
                }
                Ok(false)
            }
        }
    }

    fn promote_dep_tree(&mut self, hash: BlockId) -> Result<(), ConsensusError> {
        let mut to_explore = vec![hash];
        let mut to_promote: PreHashMap<BlockId, (Slot, u64)> = PreHashMap::default();
        while let Some(h) = to_explore.pop() {
            if to_promote.contains_key(&h) {
                continue;
            }
            if let Some(BlockStatus::WaitingForDependencies {
                header_or_block,
                unsatisfied_dependencies,
                sequence_number,
                ..
            }) = self.block_statuses.get(&h)
            {
                // promote current block
                to_promote.insert(h, (header_or_block.get_slot(), *sequence_number));
                // register dependencies for exploration
                to_explore.extend(unsatisfied_dependencies);
            }
        }

        let mut to_promote: Vec<(Slot, u64, BlockId)> = to_promote
            .into_iter()
            .map(|(h, (slot, seq))| (slot, seq, h))
            .collect();
        to_promote.sort_unstable(); // last ones should have the highest seq number
        for (_slot, _seq, h) in to_promote.into_iter() {
            if let Some(BlockStatus::WaitingForDependencies {
                sequence_number, ..
            }) = self.block_statuses.get_mut(&h)
            {
                self.sequence_counter += 1;
                *sequence_number = self.sequence_counter;
            }
        }
        Ok(())
    }

    pub fn update_state(&mut self, block_id: &BlockId, new_block_status: BlockStatus) {
        match self.block_statuses.entry(block_id.clone()) {
            Entry::Vacant(_vac) => {
                // TODO: Determine what to do ? @damip ? Panic or warn and add
            }
            Entry::Occupied(mut occ) => {
                // All possible transitions
                let value = occ.get_mut();
                match (&value, &new_block_status) {
                    // From incoming status
                    (BlockStatus::Incoming(_), _) => *value = new_block_status,

                    // From waiting for slot status
                    (BlockStatus::WaitingForSlot { .. }, BlockStatus::Incoming { .. }) => {
                        *value = new_block_status
                    }
                    (BlockStatus::WaitingForSlot { .. }, BlockStatus::Discarded { .. }) => {
                        *value = new_block_status
                    }

                    // From waiting for dependencies status
                    (BlockStatus::WaitingForDependencies { .. }, BlockStatus::Incoming { .. }) => {
                        *value = new_block_status
                    }
                    (BlockStatus::WaitingForDependencies { .. }, BlockStatus::Discarded { .. }) => {
                        *value = new_block_status
                    }

                    // From active status
                    (BlockStatus::Active { .. }, BlockStatus::Discarded { .. }) => {
                        *value = new_block_status
                    }
                    // No possibility for discarded status
                    // All other possibilities are invalid
                    (_, _) => warn!(
                        "Invalid transition from {:?} to {:?}",
                        value, new_block_status
                    ),
                }
            }
        }
    }
}
