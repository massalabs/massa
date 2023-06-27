use std::collections::hash_map::Entry;

use massa_consensus_exports::{block_status::BlockStatus, error::ConsensusError};
use massa_models::{
    block_id::BlockId,
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
};
use tracing::warn;

#[derive(Debug, Clone)]
pub struct BlocksState {
    /// Every block we know about
    block_statuses: PreHashMap<BlockId, BlockStatus>,
    /// Ids of incoming blocks/headers
    incoming_index: PreHashSet<BlockId>,
    /// Used to limit the number of waiting and discarded blocks
    sequence_counter: u64,
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

    pub fn get(&self, block_id: &BlockId) -> Option<&BlockStatus> {
        self.block_statuses.get(block_id)
    }

    pub fn get_mut(&mut self, block_id: &BlockId) -> Option<&mut BlockStatus> {
        self.block_statuses.get_mut(block_id)
    }

    pub fn sequence_counter(&self) -> u64 {
        self.sequence_counter
    }

    pub fn inc_sequence_counter(&mut self) {
        self.sequence_counter += 1
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

    pub fn active_blocks(&self) -> &PreHashSet<BlockId> {
        &self.active_index
    }

    // Internal function to update the indexes
    fn update_indexes(
        &mut self,
        block_id: &BlockId,
        old_block_status: &Option<BlockStatus>,
        new_block_status: &Option<BlockStatus>,
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
                    self.active_index.remove(block_id);
                }
            }
        }
        if let Some(new_block_status) = new_block_status {
            match new_block_status {
                BlockStatus::Incoming(_) => {
                    self.incoming_index.insert(*block_id);
                }
                BlockStatus::WaitingForSlot { .. } => {
                    self.waiting_for_slot_index.insert(*block_id);
                }
                BlockStatus::WaitingForDependencies { .. } => {
                    self.waiting_for_dependencies_index.insert(*block_id);
                }
                BlockStatus::Discarded { .. } => {
                    self.discarded_index.insert(*block_id);
                }
                BlockStatus::Active { .. } => {
                    self.active_index.insert(*block_id);
                }
            }
        }
    }

    /// - If the set did not previously contain a value, None is returned.
    /// - If the set already contained a value, Some(value) is returned.
    /// TODO: Expose entry system
    pub fn insert_block(
        &mut self,
        block_id: BlockId,
        block_status: BlockStatus,
    ) -> Result<Option<BlockStatus>, ConsensusError> {
        match self.block_statuses.entry(block_id) {
            Entry::Vacant(vac) => {
                vac.insert(block_status.clone());
                self.update_indexes(&block_id, &None, &Some(block_status));
                Ok(None)
            }
            Entry::Occupied(occ) => Ok(Some(occ.get().clone())),
        }
    }

    /// - Return Some(BlockStatus) if the block was in the set.
    /// - Return None if the block was not in the set.
    pub fn remove_block(&mut self, hash: &BlockId) -> Option<BlockStatus> {
        if let Some(block_status) = self.block_statuses.remove(hash) {
            self.update_indexes(hash, &Some(block_status.clone()), &None);
            Some(block_status)
        } else {
            None
        }
    }

    pub fn promote_dep_tree(&mut self, hash: BlockId) -> Result<(), ConsensusError> {
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

    pub fn iter(&self) -> impl Iterator<Item = (&BlockId, &BlockStatus)> + '_ {
        self.block_statuses.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&BlockId, &mut BlockStatus)> + '_ {
        self.block_statuses.iter_mut()
    }

    pub fn len(&self) -> usize {
        self.block_statuses.len()
    }

    pub fn update_block_state(
        &mut self,
        block_id: &BlockId,
        mut new_block_status: BlockStatus,
    ) -> Result<(), ConsensusError> {
        match self.block_statuses.entry(*block_id) {
            Entry::Vacant(_vac) => {
                // TODO: Determine what to do ? @damip ? Panic or warn and add
            }
            Entry::Occupied(mut occ) => {
                // All possible transitions
                let value = occ.get_mut();
                let old_value = value.clone();
                match (&value, &mut new_block_status) {
                    // From incoming status
                    (
                        BlockStatus::Incoming(_),
                        BlockStatus::WaitingForDependencies {
                            sequence_number, ..
                        },
                    ) => {
                        self.sequence_counter += 1;
                        *sequence_number = self.sequence_counter;
                        *value = new_block_status.clone();
                        self.promote_dep_tree(*block_id)?;
                    }
                    (
                        BlockStatus::Incoming(_),
                        BlockStatus::Discarded {
                            sequence_number, ..
                        },
                    ) => {
                        self.sequence_counter += 1;
                        *sequence_number = self.sequence_counter;
                        *value = new_block_status.clone();
                    }
                    (BlockStatus::Incoming(_), _) => *value = new_block_status.clone(),

                    // From waiting for slot status
                    (BlockStatus::WaitingForSlot { .. }, BlockStatus::Incoming { .. }) => {
                        *value = new_block_status.clone()
                    }
                    (
                        BlockStatus::WaitingForSlot { .. },
                        BlockStatus::Discarded {
                            sequence_number, ..
                        },
                    ) => {
                        self.sequence_counter += 1;
                        *sequence_number = self.sequence_counter;
                        *value = new_block_status.clone()
                    }

                    // From waiting for dependencies status
                    (BlockStatus::WaitingForDependencies { .. }, BlockStatus::Incoming { .. }) => {
                        *value = new_block_status.clone()
                    }
                    (
                        BlockStatus::WaitingForDependencies { .. },
                        BlockStatus::Discarded {
                            sequence_number, ..
                        },
                    ) => {
                        self.sequence_counter += 1;
                        *sequence_number = self.sequence_counter;
                        *value = new_block_status.clone()
                    }

                    // From active status
                    (BlockStatus::Active { .. }, BlockStatus::Discarded { .. }) => {
                        *value = new_block_status.clone()
                    }
                    // No possibility for discarded status
                    // All other possibilities are invalid
                    (_, _) => {
                        warn!(
                            "Invalid transition from {:?} to {:?}",
                            value, new_block_status
                        );
                        return Err(ConsensusError::InvalidTransition(format!(
                            "Invalid transition from {:?} to {:?}",
                            value, new_block_status
                        )));
                    }
                }
                self.update_indexes(block_id, &Some(old_value), &Some(new_block_status));
            }
        }
        Ok(())
    }
}
