use super::{process::BlockInfos, ConsensusState};
use massa_consensus_exports::block_status::{BlockStatus, DiscardReason, HeaderOrBlock};
use massa_logging::massa_trace;
use massa_models::{
    block_header::SecuredHeader, block_id::BlockId, prehash::PreHashSet, slot::Slot,
};
use tracing::warn;

/// Possible output of a header check
#[derive(Debug)]
pub enum HeaderCheckOutcome {
    /// it's ok and here are some useful values
    Proceed {
        /// one (parent block id, parent's period) per thread
        parents_hash_period: Vec<(BlockId, u64)>,
        /// blocks that header is incompatible with
        incompatibilities: PreHashSet<BlockId>,
        /// number of incompatibilities that are inherited from the parents
        inherited_incompatibilities_count: usize,
        /// fitness
        fitness: u64,
    },
    /// there is something wrong with that header
    Discard(DiscardReason),
    /// it must wait for its slot to be fully processed
    WaitForSlot,
    /// it must wait for these block ids to be fully processed
    WaitForDependencies(PreHashSet<BlockId>),
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum BlockCheckOutcome {
    BlockInfos(BlockInfos),
    BlockStatus(BlockStatus),
}

/// Possible outcomes of endorsements check
#[derive(Debug)]
pub enum EndorsementsCheckOutcome {
    /// Everything is ok
    Proceed,
    /// There is something wrong with that endorsement
    Discard(DiscardReason),
    /// It must wait for its slot to be fully processed
    WaitForSlot,
}

impl ConsensusState {
    // Verify that we haven't already received 2 blocks for this slot
    // If the block isn't already present two times we save it and return false
    // If the block is already present two times we return true
    pub(crate) fn detect_multistake(&mut self, header: &SecuredHeader) -> bool {
        let entry = self
            .nonfinal_active_blocks_per_slot
            .entry(header.content.slot)
            .or_default();
        if !entry.contains(&header.id) {
            if entry.len() > 1 && !self.wishlist.contains_key(&header.id) {
                warn!(
                    "received more than 2 blocks for slot {}",
                    header.content.slot
                );
                return true;
            } else {
                entry.insert(header.id);
            }
        }
        false
    }

    /// Check if the header is valid and if it could be processed when we will receive the full block
    pub(crate) fn convert_block_header(
        &mut self,
        block_id: BlockId,
        header: SecuredHeader,
        current_slot: Option<Slot>,
    ) -> Option<BlockStatus> {
        let header_outcome = self.check_header(&block_id, &header, current_slot);
        match header_outcome {
            HeaderCheckOutcome::Proceed { .. } => {
                if self.detect_multistake(&header) {
                    return None;
                }
                // set as waiting dependencies
                let mut dependencies = PreHashSet::<BlockId>::default();
                dependencies.insert(block_id); // add self as unsatisfied
                massa_trace!(
                    "consensus.block_graph.process.incoming_header.waiting_for_self",
                    { "block_id": block_id }
                );
                Some(BlockStatus::WaitingForDependencies {
                    header_or_block: HeaderOrBlock::Header(header),
                    unsatisfied_dependencies: dependencies,
                    sequence_number: self.blocks_state.sequence_counter(),
                })
            }
            HeaderCheckOutcome::WaitForDependencies(mut dependencies) => {
                if self.detect_multistake(&header) {
                    return None;
                }
                // set as waiting dependencies
                dependencies.insert(block_id); // add self as unsatisfied
                Some(BlockStatus::WaitingForDependencies {
                    header_or_block: HeaderOrBlock::Header(header),
                    unsatisfied_dependencies: dependencies,
                    sequence_number: self.blocks_state.sequence_counter(),
                })
            }
            HeaderCheckOutcome::WaitForSlot => {
                if self.detect_multistake(&header) {
                    return None;
                }
                Some(BlockStatus::WaitingForSlot(HeaderOrBlock::Header(header)))
            }
            HeaderCheckOutcome::Discard(reason) => {
                Some(self.convert_to_discard_block_header(reason, block_id, header))
            }
        }
    }

    /// Store in our indexes that we discarded this block or block header
    ///
    /// # Arguments:
    /// `reason`: Read of the discard
    /// `block_id`: ID of the block
    /// `header`: header to save
    fn convert_to_discard_block_header(
        &mut self,
        reason: DiscardReason,
        block_id: BlockId,
        header: SecuredHeader,
    ) -> BlockStatus {
        self.maybe_note_attack_attempt(&reason, &block_id);
        massa_trace!("consensus.block_graph.process.incoming_header.discarded", {"block_id": block_id, "reason": reason});
        // count stales
        if reason == DiscardReason::Stale {
            self.new_stale_blocks.insert(
                block_id,
                (header.content_creator_address, header.content.slot),
            );
        }
        // discard
        BlockStatus::Discarded {
            slot: header.content.slot,
            creator: header.content_creator_address,
            parents: header.content.parents,
            reason,
            sequence_number: self.blocks_state.sequence_counter(),
        }
    }

    /// Process an incoming header.
    ///
    /// Checks performed:
    /// - Number of parents matches thread count.
    /// - Slot above 0.
    /// - Valid thread.
    /// - Check that the block is older than the latest final one in thread.
    /// - Check if it was the creator's turn to create this block.
    /// - Check parents are present.
    /// - Check the topological consistency of the parents.
    /// - Check endorsements.
    /// - Check thread incompatibility test.
    /// - Check grandpa incompatibility test.
    /// - Check if the block is incompatible with a parent.
    /// - Check if the block is incompatible with a final block.
    pub(crate) fn check_header(
        &self,
        block_id: &BlockId,
        header: &SecuredHeader,
        current_slot: Option<Slot>,
    ) -> HeaderCheckOutcome {
        massa_trace!("consensus.block_graph.check_header", {
            "block_id": block_id
        });
        let mut parents: Vec<(BlockId, u64)> =
            Vec::with_capacity(self.config.thread_count as usize);
        let mut incomp = PreHashSet::<BlockId>::default();
        let mut missing_deps = PreHashSet::<BlockId>::default();
        let creator_addr = header.content_creator_address;

        // check that is older than the latest final block in that thread
        // Note: this excludes genesis blocks
        if header.content.slot.period
            <= self.latest_final_blocks_periods[header.content.slot.thread as usize].1
        {
            return HeaderCheckOutcome::Discard(DiscardReason::Stale);
        }

        // check if it was the creator's turn to create this block
        // (step 1 in consensus/pos.md)
        let slot_draw_address = match self
            .channels
            .selector_controller
            .get_producer(header.content.slot)
        {
            Ok(draw) => draw,
            Err(_) => return HeaderCheckOutcome::WaitForSlot, // TODO properly handle PoS errors
        };
        if creator_addr != slot_draw_address {
            // it was not the creator's turn to create a block for this slot
            return HeaderCheckOutcome::Discard(DiscardReason::Invalid(format!(
                "Bad creator turn for the slot:{}",
                header.content.slot
            )));
        }

        // check if block is in the future: queue it
        // note: do it after testing signature + draw to prevent queue flooding/DoS
        // note: Some(x) > None
        if Some(header.content.slot) > current_slot {
            return HeaderCheckOutcome::WaitForSlot;
        }

        // Note: here we will check if we already have a block for that slot
        // and if someone double staked, they will be denounced

        // list parents and ensure they are present
        let parent_set: PreHashSet<BlockId> = header.content.parents.iter().copied().collect();
        for parent_thread in 0u8..self.config.thread_count {
            let parent_hash = header.content.parents[parent_thread as usize];
            match self.blocks_state.get(&parent_hash) {
                Some(BlockStatus::Discarded { reason, .. }) => {
                    // parent is discarded
                    return HeaderCheckOutcome::Discard(match reason {
                        DiscardReason::Invalid(invalid_reason) => DiscardReason::Invalid(format!(
                            "discarded because a parent was discarded for the following reason: {}",
                            invalid_reason
                        )),
                        r => r.clone(),
                    });
                }
                Some(BlockStatus::Active {
                    a_block: parent, ..
                }) => {
                    // parent is active

                    // check that the parent is from an earlier slot in the right thread
                    if parent.slot.thread != parent_thread || parent.slot >= header.content.slot {
                        return HeaderCheckOutcome::Discard(DiscardReason::Invalid(format!(
                            "Bad parent {} in thread:{} or slot:{} for {}.",
                            parent_hash, parent_thread, parent.slot, header.content.slot
                        )));
                    }

                    // inherit parent incompatibilities
                    // and ensure parents are mutually compatible
                    if let Some(p_incomp) = self.gi_head.get(&parent_hash) {
                        if !p_incomp.is_disjoint(&parent_set) {
                            return HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                                "Parent not mutually compatible".to_string(),
                            ));
                        }
                        incomp.extend(p_incomp);
                    }

                    parents.push((parent_hash, parent.slot.period));
                }
                _ => {
                    // parent is missing or queued
                    if self.genesis_hashes.contains(&parent_hash) {
                        // forbid depending on discarded genesis block
                        return HeaderCheckOutcome::Discard(DiscardReason::Stale);
                    }
                    missing_deps.insert(parent_hash);
                }
            }
        }
        if !missing_deps.is_empty() {
            return HeaderCheckOutcome::WaitForDependencies(missing_deps);
        }
        let inherited_incomp_count = incomp.len();

        // check the topological consistency of the parents
        {
            let mut gp_max_slots = vec![0u64; self.config.thread_count as usize];
            for parent_i in 0..self.config.thread_count {
                let (parent_h, parent_period) = parents[parent_i as usize];
                let parent = match self.blocks_state.get(&parent_h) {
                    Some(BlockStatus::Active {
                        a_block,
                        storage: _,
                    }) => a_block,
                    _ => {
                        panic!(
                            "inconsistency inside block statuses searching parent {} of block {}",
                            parent_h, block_id
                        )
                    }
                };
                if parent_period < gp_max_slots[parent_i as usize] {
                    // a parent is earlier than a block known by another parent in that thread
                    return HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                        "a parent is earlier than a block known by another parent in that thread"
                            .to_string(),
                    ));
                }
                gp_max_slots[parent_i as usize] = parent_period;
                if parent_period == self.config.last_start_period {
                    // genesis
                    continue;
                }
                for gp_i in 0..self.config.thread_count {
                    if gp_i == parent_i {
                        continue;
                    }
                    let gp_h = parent.parents[gp_i as usize].0;
                    match self.blocks_state.get(&gp_h) {
                        // this grandpa is discarded
                        Some(BlockStatus::Discarded { reason, .. }) => {
                            return HeaderCheckOutcome::Discard(reason.clone());
                        }
                        // this grandpa is active
                        Some(BlockStatus::Active { a_block: gp, .. }) => {
                            if gp.slot.period > gp_max_slots[gp_i as usize] {
                                if gp_i < parent_i {
                                    return HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                                        "grandpa error: gp_i < parent_i".to_string(),
                                    ));
                                }
                                gp_max_slots[gp_i as usize] = gp.slot.period;
                            }
                        }
                        // this grandpa is missing, assume stale
                        _ => return HeaderCheckOutcome::Discard(DiscardReason::Stale),
                    }
                }
            }
        }

        // check endorsements
        match self.check_endorsements(header) {
            EndorsementsCheckOutcome::Proceed => {}
            EndorsementsCheckOutcome::Discard(reason) => {
                return HeaderCheckOutcome::Discard(reason)
            }
            EndorsementsCheckOutcome::WaitForSlot => return HeaderCheckOutcome::WaitForSlot,
        }

        // incompatibility test
        {
            // list all ancestors until we reach a final block (included)
            let mut ancestor_ids: PreHashSet<BlockId> = Default::default();
            let mut to_traverse = header.content.parents.clone();
            let mut earliest_visited_periods: Vec<u64> = parents.iter().map(|(_, p)| *p).collect();
            while let Some(ancestor_id) = to_traverse.pop() {
                if !ancestor_ids.insert(ancestor_id) {
                    // ancestor already visited
                    continue;
                }

                // get ancestor
                if let Some(BlockStatus::Active {
                    a_block: ancestor, ..
                }) = self.blocks_state.get(&ancestor_id)
                {
                    // update earliest_visited_periods
                    earliest_visited_periods[ancestor.slot.thread as usize] = std::cmp::min(
                        earliest_visited_periods[ancestor.slot.thread as usize],
                        ancestor.slot.period,
                    );

                    // Continue traversing if not final.
                    // Note that we use an optimization:
                    // only traverse in the same thread,
                    // which is OK because of the parent time consistency check done before
                    if !ancestor.is_final {
                        if let Some(parent_id_slot) =
                            ancestor.parents.get(ancestor.slot.thread as usize)
                        {
                            to_traverse.push(parent_id_slot.0);
                        }
                    }
                }
            }

            // check incompatibilities with non-ancestors
            for traversed_id in self.blocks_state.active_blocks() {
                // skip if the traversed block is an ancestor of the incoming block
                if ancestor_ids.contains(traversed_id) {
                    continue;
                }

                // get information on the traversed block
                let traversed_block = match self.blocks_state.get(traversed_id) {
                    Some(BlockStatus::Active { a_block, .. }) => a_block,
                    _ => {
                        panic!(
                            "inconsistency inside block statuses searching traversed {} of block {}",
                            traversed_id, block_id
                        )
                    }
                };

                // skip if the block is before the earliest listed ancestors of that thread
                if traversed_block.slot.period
                    < earliest_visited_periods[traversed_block.slot.thread as usize]
                {
                    continue;
                }

                // check incompatibility
                let incompatible = match header.content.slot.cmp(&traversed_block.slot) {
                    // the traversed block is at the same slot as the incoming block => they are incompatible
                    std::cmp::Ordering::Equal => true,
                    // the time interval between the traversed block and the incoming block is higher or equal to t0 => they are incompatible
                    std::cmp::Ordering::Greater => {
                        header
                            .content
                            .slot
                            .slots_since(&traversed_block.slot, self.config.thread_count)
                            .expect("arithmetic overflow on slots while checking incompatibilities")
                            >= self.config.thread_count as u64
                    }
                    // the time interval between the traversed block and the incoming block is higher or equal to t0 => they are incompatible
                    std::cmp::Ordering::Less => {
                        traversed_block
                            .slot
                            .slots_since(&header.content.slot, self.config.thread_count)
                            .expect("arithmetic overflow on slots while checking incompatibilities")
                            >= self.config.thread_count as u64
                    }
                };

                // if incompatible, add to incompatibilities and exit early if the incoming header is incompatible with a final block
                if incompatible {
                    if traversed_block.is_final {
                        return HeaderCheckOutcome::Discard(DiscardReason::Stale);
                    }
                    incomp.extend(self.get_active_block_and_descendants(traversed_id));
                }
            }
        }

        // check if the block is incompatible with a parent
        if !incomp.is_disjoint(&parents.iter().map(|(h, _p)| *h).collect()) {
            return HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                "Block incompatible with a parent".to_string(),
            ));
        }

        // check if the block is incompatible with a final block
        if !incomp.is_disjoint(
            &self
                .blocks_state
                .active_blocks()
                .iter()
                .filter_map(|h| {
                    if let Some(BlockStatus::Active { a_block: a, .. }) = self.blocks_state.get(h) {
                        if a.is_final {
                            return Some(*h);
                        }
                    }
                    None
                })
                .collect(),
        ) {
            return HeaderCheckOutcome::Discard(DiscardReason::Stale);
        }
        massa_trace!("consensus.block_graph.check_header.ok", {
            "block_id": block_id
        });

        HeaderCheckOutcome::Proceed {
            parents_hash_period: parents,
            incompatibilities: incomp,
            inherited_incompatibilities_count: inherited_incomp_count,
            fitness: header.get_fitness(),
        }
    }

    /// check endorsements:
    /// * endorser was selected for that (slot, index)
    /// * endorsed slot is `parent_in_own_thread` slot
    pub fn check_endorsements(&self, header: &SecuredHeader) -> EndorsementsCheckOutcome {
        // check endorsements
        let endorsement_draws = match self
            .channels
            .selector_controller
            .get_selection(header.content.slot)
        {
            Ok(sel) => sel.endorsements,
            Err(_) => return EndorsementsCheckOutcome::WaitForSlot,
        };
        for endorsement in header.content.endorsements.iter() {
            // check that the draw is correct
            if endorsement.content_creator_address
                != endorsement_draws[endorsement.content.index as usize]
            {
                return EndorsementsCheckOutcome::Discard(DiscardReason::Invalid(format!(
                    "endorser draw mismatch for header in slot: {}",
                    header.content.slot
                )));
            }

            // note that the following aspects are checked in protocol
            // * signature
            // * index reuse
            // * slot matching the block's
            // * the endorsed block is the containing block's parent
        }

        EndorsementsCheckOutcome::Proceed
    }
}
