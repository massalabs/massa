use core::panic;

use massa_consensus_exports::block_status::{BlockStatus, BlockStatusId};
use massa_models::{
    block_id::BlockId,
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
};

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
    /// Initialize the `BlocksState` structure
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

    /// Get a reference on a `BlockStatus` from a `BlockId`
    pub fn get(&self, block_id: &BlockId) -> Option<&BlockStatus> {
        self.block_statuses.get(block_id)
    }

    /// Get a mutable reference on a `BlockStatus` from a `BlockId`
    pub fn get_mut(&mut self, block_id: &BlockId) -> Option<&mut BlockStatus> {
        self.block_statuses.get_mut(block_id)
    }

    /// Get the sequence counter
    pub fn sequence_counter(&self) -> u64 {
        self.sequence_counter
    }

    /// Get a reference on the list of all blocks stored with the status `Incoming`
    pub fn incoming_blocks(&self) -> &PreHashSet<BlockId> {
        &self.incoming_index
    }

    /// Get a reference on the list of all blocks stored with the status `WaitingForSlot`
    pub fn waiting_for_slot_blocks(&self) -> &PreHashSet<BlockId> {
        &self.waiting_for_slot_index
    }

    /// Get a reference on the list of all blocks stored with the status `WaitingForDependencies`
    pub fn waiting_for_dependencies_blocks(&self) -> &PreHashSet<BlockId> {
        &self.waiting_for_dependencies_index
    }

    /// Get a reference on the list of all blocks stored with the status `Discarded`
    pub fn discarded_blocks(&self) -> &PreHashSet<BlockId> {
        &self.discarded_index
    }

    /// Get a reference on the list of all blocks stored with the status `Active`
    pub fn active_blocks(&self) -> &PreHashSet<BlockId> {
        &self.active_index
    }

    // Internal function to update the indexes
    fn update_indexes(
        &mut self,
        block_id: &BlockId,
        old_block_status: Option<&BlockStatusId>,
        new_block_status: Option<&BlockStatusId>,
    ) {
        if let Some(old_block_status) = old_block_status {
            match old_block_status {
                BlockStatusId::Incoming => {
                    self.incoming_index.remove(block_id);
                }
                BlockStatusId::WaitingForSlot => {
                    self.waiting_for_slot_index.remove(block_id);
                }
                BlockStatusId::WaitingForDependencies => {
                    self.waiting_for_dependencies_index.remove(block_id);
                }
                BlockStatusId::Discarded => {
                    self.discarded_index.remove(block_id);
                }
                BlockStatusId::Active => {
                    self.active_index.remove(block_id);
                }
            }
        }
        if let Some(new_block_status) = new_block_status {
            match new_block_status {
                BlockStatusId::Incoming => {
                    self.incoming_index.insert(*block_id);
                }
                BlockStatusId::WaitingForSlot => {
                    self.waiting_for_slot_index.insert(*block_id);
                }
                BlockStatusId::WaitingForDependencies => {
                    self.waiting_for_dependencies_index.insert(*block_id);
                }
                BlockStatusId::Discarded => {
                    self.discarded_index.insert(*block_id);
                }
                BlockStatusId::Active => {
                    self.active_index.insert(*block_id);
                }
            }
        }
    }

    /// Promote a block id and all of its dependencies to be the first in the sequence
    fn promote_dep_tree(&mut self, hash: BlockId) {
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
    }

    /// Get an iterator over all the blocks stored in the `BlocksState`
    pub fn iter(&self) -> impl Iterator<Item = (&BlockId, &BlockStatus)> + '_ {
        self.block_statuses.iter()
    }

    /// Get a mutable iterator over all the blocks stored in the `BlocksState`
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&BlockId, &mut BlockStatus)> + '_ {
        self.block_statuses.iter_mut()
    }

    /// Get the number of blocks stored in the `BlocksState`
    pub fn len(&self) -> usize {
        self.block_statuses.len()
    }

    /// Change the state of a block
    /// Steps are:
    /// 1. Remove the block from state
    /// 2. Call the callback function with the removed state (or None if block wasn't in the `BlocksState`) as parameter and retrieve the new state (or None if we don't want to add the block again in the `BlocksState`)
    /// 3. Verify that the transition is valid
    /// 4. Insert the new state in the `BlocksState`
    pub fn transition_map<
        F: FnOnce(Option<BlockStatus>, &mut PreHashMap<BlockId, BlockStatus>) -> Option<BlockStatus>,
    >(
        &mut self,
        block_id: &BlockId,
        callback: F,
    ) {
        match self.block_statuses.remove(block_id) {
            Some(block) => {
                let old_state_id = BlockStatusId::from(&block);
                self.update_indexes(block_id, Some(&old_state_id), None);
                let Some(mut new_state) = callback(Some(block), &mut self.block_statuses) else { return; };
                let new_state_id = BlockStatusId::from(&new_state);
                match (&old_state_id, &new_state_id) {
                    // From incoming status
                    (BlockStatusId::Incoming, BlockStatusId::WaitingForDependencies) => {
                        self.block_statuses.insert(*block_id, new_state);
                        self.promote_dep_tree(*block_id);
                    }
                    (BlockStatusId::Incoming, BlockStatusId::Discarded) => {
                        if let BlockStatus::Discarded {
                            sequence_number, ..
                        } = &mut new_state
                        {
                            self.sequence_counter += 1;
                            *sequence_number = self.sequence_counter;
                        } else {
                            panic!("Unexpected block status for {}", block_id);
                        }
                        self.block_statuses.insert(*block_id, new_state);
                    }
                    (BlockStatusId::Incoming, _) => {
                        self.block_statuses.insert(*block_id, new_state);
                    }

                    // From waiting for slot status
                    (BlockStatusId::WaitingForSlot, BlockStatusId::Incoming) => {
                        self.block_statuses.insert(*block_id, new_state);
                    }
                    (BlockStatusId::WaitingForSlot, BlockStatusId::Discarded) => {
                        if let BlockStatus::Discarded {
                            sequence_number, ..
                        } = &mut new_state
                        {
                            self.sequence_counter += 1;
                            *sequence_number = self.sequence_counter;
                        } else {
                            panic!("Unexpected block status for {}", block_id);
                        }
                        self.block_statuses.insert(*block_id, new_state);
                    }
                    (BlockStatusId::WaitingForSlot, BlockStatusId::WaitingForSlot) => {
                        self.block_statuses.insert(*block_id, new_state);
                    }

                    // From waiting for dependencies status
                    (BlockStatusId::WaitingForDependencies, BlockStatusId::Incoming) => {
                        self.block_statuses.insert(*block_id, new_state);
                    }
                    (BlockStatusId::WaitingForDependencies, BlockStatusId::Discarded) => {
                        if let BlockStatus::Discarded {
                            sequence_number, ..
                        } = &mut new_state
                        {
                            self.sequence_counter += 1;
                            *sequence_number = self.sequence_counter;
                        } else {
                            panic!("Unexpected block status for {}", block_id);
                        }
                        self.block_statuses.insert(*block_id, new_state);
                    }
                    (
                        BlockStatusId::WaitingForDependencies,
                        BlockStatusId::WaitingForDependencies,
                    ) => {
                        self.block_statuses.insert(*block_id, new_state);
                        self.promote_dep_tree(*block_id);
                    }

                    // From active status
                    (BlockStatusId::Active, BlockStatusId::Discarded) => {
                        self.block_statuses.insert(*block_id, new_state);
                    }
                    (BlockStatusId::Active, BlockStatusId::Active) => {
                        self.block_statuses.insert(*block_id, new_state);
                    }

                    // From discarded status
                    (BlockStatusId::Discarded, BlockStatusId::Discarded) => {
                        if let BlockStatus::Discarded {
                            sequence_number, ..
                        } = &mut new_state
                        {
                            self.sequence_counter += 1;
                            *sequence_number = self.sequence_counter;
                        } else {
                            panic!("Unexpected block status for {}", block_id);
                        }
                        self.block_statuses.insert(*block_id, new_state);
                    }
                    // All other possibilities are invalid
                    (old_state_id, new_state_id) => {
                        panic!(
                            "Invalid transition from {:?} to {:?} for block {}",
                            old_state_id, new_state_id, block_id
                        );
                    }
                }
                self.update_indexes(block_id, None, Some(&new_state_id));
            }
            None => {
                let new_state = callback(None, &mut self.block_statuses);
                if let Some(new_state) = new_state {
                    let state = BlockStatusId::from(&new_state);
                    if state != BlockStatusId::Incoming && state != BlockStatusId::Active {
                        panic!(
                            "Invalid transition from None to {:?} for block {}",
                            state, block_id
                        );
                    }
                    self.block_statuses.insert(*block_id, new_state);
                    self.update_indexes(block_id, None, Some(&state));
                }
            }
        };
    }
}
