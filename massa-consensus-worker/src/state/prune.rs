use massa_consensus_exports::{
    block_status::{BlockStatus, DiscardReason, HeaderOrBlock},
    error::ConsensusError,
};
use massa_logging::massa_trace;
use massa_models::{
    active_block::ActiveBlock,
    block_id::BlockId,
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
};
use tracing::debug;

use super::ConsensusState;

impl ConsensusState {
    /// prune active blocks and return final blocks, return discarded final blocks
    fn prune_active(&mut self) -> Result<PreHashMap<BlockId, ActiveBlock>, ConsensusError> {
        // list required active blocks
        let mut retain_active: PreHashSet<BlockId> = self.list_required_active_blocks(None)?;

        // retain extra history according to the config
        // this is useful to avoid desync on temporary connection loss
        for a_block in self.active_index.iter() {
            if let Some(BlockStatus::Active {
                a_block: active_block,
                storage,
            }) = self.block_statuses.get_mut(a_block)
            {
                let (_b_id, latest_final_period) =
                    self.latest_final_blocks_periods[active_block.slot.thread as usize];

                if active_block.slot.period
                    >= latest_final_period
                        .saturating_sub(self.config.force_keep_final_periods_without_ops)
                {
                    retain_active.insert(*a_block);
                    self.active_index_without_ops.remove(a_block);
                } else {
                    if active_block.slot.period
                        >= latest_final_period.saturating_sub(self.config.force_keep_final_periods)
                    {
                        if !self.active_index_without_ops.contains(a_block) {
                            self.active_index_without_ops.insert(*a_block);
                            storage.drop_operation_refs(&storage.get_op_refs().clone());
                        }
                    }
                }
            }
        }

        // remove unused final active blocks
        let mut discarded_finals: PreHashMap<BlockId, ActiveBlock> = PreHashMap::default();
        let to_remove: Vec<BlockId> = self
            .active_index
            .difference(&retain_active)
            .copied()
            .collect();
        for discard_active_h in to_remove {
            let block_slot;
            let block_creator;
            let block_parents;
            {
                let read_blocks = self.storage.read_blocks();
                let block = read_blocks.get(&discard_active_h).ok_or_else(|| {
                    ConsensusError::MissingBlock(format!(
                        "missing block when removing unused final active blocks: {}",
                        discard_active_h
                    ))
                })?;
                block_slot = block.content.header.content.slot;
                block_creator = block.content_creator_address;
                block_parents = block.content.header.content.parents.clone();
            };

            let discarded_active = if let Some(BlockStatus::Active {
                a_block: discarded_active,
                ..
            }) = self.block_statuses.remove(&discard_active_h)
            {
                self.active_index.remove(&discard_active_h);
                discarded_active
            } else {
                return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses pruning and removing unused final active blocks - {} is missing", discard_active_h)));
            };

            // remove from parent's children
            for (parent_h, _parent_period) in discarded_active.parents.iter() {
                if let Some(BlockStatus::Active {
                    a_block: parent_active_block,
                    ..
                }) = self.block_statuses.get_mut(parent_h)
                {
                    parent_active_block.children[discarded_active.slot.thread as usize]
                        .remove(&discard_active_h);
                }
            }

            massa_trace!("consensus.block_graph.prune_active", {"hash": discard_active_h, "reason": DiscardReason::Final});

            // mark as final
            self.block_statuses.insert(
                discard_active_h,
                BlockStatus::Discarded {
                    slot: block_slot,
                    creator: block_creator,
                    parents: block_parents,
                    reason: DiscardReason::Final,
                    sequence_number: {
                        self.sequence_counter += 1;
                        self.sequence_counter
                    },
                },
            );
            self.discarded_index.insert(discard_active_h);

            discarded_finals.insert(discard_active_h, *discarded_active);
        }

        Ok(discarded_finals)
    }

    // Keep only a certain (`config.max_future_processing_blocks`) number of blocks that have slots in the future
    // to avoid high memory consumption
    fn prune_slot_waiting(&mut self) {
        if self.waiting_for_slot_index.len() <= self.config.max_future_processing_blocks {
            return;
        }
        let mut slot_waiting: Vec<(Slot, BlockId)> = self
            .waiting_for_slot_index
            .iter()
            .filter_map(|block_id| {
                if let Some(BlockStatus::WaitingForSlot(header_or_block)) =
                    self.block_statuses.get(block_id)
                {
                    return Some((header_or_block.get_slot(), *block_id));
                }
                None
            })
            .collect();
        slot_waiting.sort_unstable();
        let len_slot_waiting = slot_waiting.len();
        (self.config.max_future_processing_blocks..len_slot_waiting).for_each(|idx| {
            let (_slot, block_id) = &slot_waiting[idx];
            self.block_statuses.remove(block_id);
            self.waiting_for_slot_index.remove(block_id);
        });
    }

    // Keep only a certain (`config.max_discarded_blocks`) number of blocks that are discarded
    // to avoid high memory consumption
    fn prune_discarded(&mut self) -> Result<(), ConsensusError> {
        if self.discarded_index.len() <= self.config.max_discarded_blocks {
            return Ok(());
        }
        let mut discard_hashes: Vec<(u64, BlockId)> = self
            .discarded_index
            .iter()
            .filter_map(|block_id| {
                if let Some(BlockStatus::Discarded {
                    sequence_number, ..
                }) = self.block_statuses.get(block_id)
                {
                    return Some((*sequence_number, *block_id));
                }
                None
            })
            .collect();
        discard_hashes.sort_unstable();
        discard_hashes.truncate(self.discarded_index.len() - self.config.max_discarded_blocks);
        for (_, block_id) in discard_hashes.iter() {
            self.block_statuses.remove(block_id);
            self.discarded_index.remove(block_id);
        }
        Ok(())
    }

    fn prune_waiting_for_dependencies(&mut self) -> Result<(), ConsensusError> {
        let mut to_discard: PreHashMap<BlockId, Option<DiscardReason>> = PreHashMap::default();
        let mut to_keep: PreHashMap<BlockId, (u64, Slot)> = PreHashMap::default();

        // list items that are older than the latest final blocks in their threads or have deps that are discarded
        {
            for block_id in self.waiting_for_dependencies_index.iter() {
                if let Some(BlockStatus::WaitingForDependencies {
                    header_or_block,
                    unsatisfied_dependencies,
                    sequence_number,
                }) = self.block_statuses.get(block_id)
                {
                    // has already discarded dependencies => discard (choose worst reason)
                    let mut discard_reason = None;
                    let mut discarded_dep_found = false;
                    for dep in unsatisfied_dependencies.iter() {
                        if let Some(BlockStatus::Discarded { reason, .. }) =
                            self.block_statuses.get(dep)
                        {
                            discarded_dep_found = true;
                            match reason {
                                DiscardReason::Invalid(reason) => {
                                    discard_reason = Some(DiscardReason::Invalid(format!("discarded because depend on block:{} that has discard reason:{}", block_id, reason)));
                                    break;
                                }
                                DiscardReason::Stale => discard_reason = Some(DiscardReason::Stale),
                                DiscardReason::Final => discard_reason = Some(DiscardReason::Stale),
                            }
                        }
                    }
                    if discarded_dep_found {
                        to_discard.insert(*block_id, discard_reason);
                        continue;
                    }

                    // is at least as old as the latest final block in its thread => discard as stale
                    let slot = header_or_block.get_slot();
                    if slot.period <= self.latest_final_blocks_periods[slot.thread as usize].1 {
                        to_discard.insert(*block_id, Some(DiscardReason::Stale));
                        continue;
                    }

                    // otherwise, mark as to_keep
                    to_keep.insert(*block_id, (*sequence_number, header_or_block.get_slot()));
                }
            }
        }

        // discard in chain and because of limited size
        while !to_keep.is_empty() {
            // mark entries as to_discard and remove them from to_keep
            for (hash, _old_order) in to_keep.clone().into_iter() {
                if let Some(BlockStatus::WaitingForDependencies {
                    unsatisfied_dependencies,
                    ..
                }) = self.block_statuses.get(&hash)
                {
                    // has dependencies that will be discarded => discard (choose worst reason)
                    let mut discard_reason = None;
                    let mut dep_to_discard_found = false;
                    for dep in unsatisfied_dependencies.iter() {
                        if let Some(reason) = to_discard.get(dep) {
                            dep_to_discard_found = true;
                            match reason {
                                Some(DiscardReason::Invalid(reason)) => {
                                    discard_reason = Some(DiscardReason::Invalid(format!("discarded because depend on block:{} that has discard reason:{}", hash, reason)));
                                    break;
                                }
                                Some(DiscardReason::Stale) => {
                                    discard_reason = Some(DiscardReason::Stale)
                                }
                                Some(DiscardReason::Final) => {
                                    discard_reason = Some(DiscardReason::Stale)
                                }
                                None => {} // leave as None
                            }
                        }
                    }
                    if dep_to_discard_found {
                        to_keep.remove(&hash);
                        to_discard.insert(hash, discard_reason);
                        continue;
                    }
                }
            }

            // remove worst excess element
            if to_keep.len() > self.config.max_dependency_blocks {
                let remove_elt = to_keep
                    .iter()
                    .filter_map(|(hash, _old_order)| {
                        if let Some(BlockStatus::WaitingForDependencies {
                            header_or_block,
                            sequence_number,
                            ..
                        }) = self.block_statuses.get(hash)
                        {
                            return Some((sequence_number, header_or_block.get_slot(), *hash));
                        }
                        None
                    })
                    .min();
                if let Some((_seq_num, _slot, hash)) = remove_elt {
                    to_keep.remove(&hash);
                    to_discard.insert(hash, None);
                    continue;
                }
            }

            // nothing happened: stop loop
            break;
        }

        // transition states to Discarded if there is a reason, otherwise just drop
        for (block_id, reason_opt) in to_discard.drain() {
            if let Some(BlockStatus::WaitingForDependencies {
                header_or_block, ..
            }) = self.block_statuses.remove(&block_id)
            {
                self.waiting_for_dependencies_index.remove(&block_id);
                let header = match header_or_block {
                    HeaderOrBlock::Header(h) => h,
                    HeaderOrBlock::Block { id: block_id, .. } => self
                        .storage
                        .read_blocks()
                        .get(&block_id)
                        .ok_or_else(|| {
                            ConsensusError::MissingBlock(format!(
                                "missing block when pruning waiting for deps: {}",
                                block_id
                            ))
                        })?
                        .content
                        .header
                        .clone(),
                };
                massa_trace!("consensus.block_graph.prune_waiting_for_dependencies", {"hash": block_id, "reason": reason_opt});

                if let Some(reason) = reason_opt {
                    // add to stats if reason is Stale
                    if reason == DiscardReason::Stale {
                        self.new_stale_blocks.insert(
                            block_id,
                            (header.content_creator_address, header.content.slot),
                        );
                    }
                    // transition to Discarded only if there is a reason
                    self.block_statuses.insert(
                        block_id,
                        BlockStatus::Discarded {
                            slot: header.content.slot,
                            creator: header.content_creator_address,
                            parents: header.content.parents.clone(),
                            reason,
                            sequence_number: {
                                self.sequence_counter += 1;
                                self.sequence_counter
                            },
                        },
                    );
                    self.discarded_index.insert(block_id);
                }
            }
        }

        Ok(())
    }

    /// Clear the cache of blocks indexed by slot.
    /// Slot are not saved anymore, when the block in the same thread with a equal or greater period is finalized.
    fn prune_nonfinal_blocks_per_slot(&mut self) {
        self.nonfinal_active_blocks_per_slot
            .retain(|s, _| s.period > self.latest_final_blocks_periods[s.thread as usize].1);
    }

    /// Clear all the caches and blocks waiting to be processed to avoid too much memory usage.
    pub fn prune(&mut self) -> Result<(), ConsensusError> {
        let before = self.max_cliques.len();
        // Step 1: discard final blocks that are not useful to the graph anymore and return them
        self.prune_active()?;

        // Step 2: prune slot waiting blocks
        self.prune_slot_waiting();

        // Step 3: prune dependency waiting blocks
        self.prune_waiting_for_dependencies()?;

        // Step 4: prune discarded
        self.prune_discarded()?;

        // Step 5: prune nonfinal blocks per slot
        self.prune_nonfinal_blocks_per_slot();

        let after = self.max_cliques.len();
        if before != after {
            debug!(
                "clique number went from {} to {} after pruning",
                before, after
            );
        }

        Ok(())
    }
}
