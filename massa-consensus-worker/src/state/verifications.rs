use super::ConsensusState;

use massa_consensus_exports::{
    block_status::{BlockStatus, DiscardReason},
    error::ConsensusError,
};
use massa_logging::massa_trace;
use massa_models::{
    block_header::SecuredHeader, block_id::BlockId, prehash::PreHashSet,
    slot::Slot,
};

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
    /// Process an incoming header.
    ///
    /// Checks performed:
    /// - Number of parents matches thread count.
    /// - Slot above 0.
    /// - Valid thread.
    /// - Check that the block is older than the latest final one in thread.
    /// - Check that the block slot is not too much into the future,
    ///   as determined by the configuration `future_block_processing_max_periods`.
    /// - Check if it was the creator's turn to create this block.
    /// - TODO: check for double staking.
    /// - Check parents are present.
    /// - Check the topological consistency of the parents.
    /// - Check endorsements.
    /// - Check thread incompatibility test.
    /// - Check grandpa incompatibility test.
    /// - Check if the block is incompatible with a parent.
    /// - Check if the block is incompatible with a final block.
    pub fn check_header(
        &self,
        block_id: &BlockId,
        header: &SecuredHeader,
        current_slot: Option<Slot>,
        read_shared_state: &ConsensusState,
    ) -> Result<HeaderCheckOutcome, ConsensusError> {
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
            <= read_shared_state.latest_final_blocks_periods[header.content.slot.thread as usize].1
        {
            return Ok(HeaderCheckOutcome::Discard(DiscardReason::Stale));
        }

        // check if block slot is too much in the future
        if let Some(cur_slot) = current_slot {
            if header.content.slot.period
                > cur_slot
                    .period
                    .saturating_add(self.config.future_block_processing_max_periods)
            {
                return Ok(HeaderCheckOutcome::WaitForSlot);
            }
        }

        // check if it was the creator's turn to create this block
        // (step 1 in consensus/pos.md)
        let slot_draw_address = match self
            .channels
            .selector_controller
            .get_producer(header.content.slot)
        {
            Ok(draw) => draw,
            Err(_) => return Ok(HeaderCheckOutcome::WaitForSlot), // TODO properly handle PoS errors
        };
        if creator_addr != slot_draw_address {
            // it was not the creator's turn to create a block for this slot
            return Ok(HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                format!("Bad creator turn for the slot:{}", header.content.slot),
            )));
        }

        // check if block is in the future: queue it
        // note: do it after testing signature + draw to prevent queue flooding/DoS
        // note: Some(x) > None
        if Some(header.content.slot) > current_slot {
            return Ok(HeaderCheckOutcome::WaitForSlot);
        }

        // Note: here we will check if we already have a block for that slot
        // and if someone double staked, they will be denounced

        // list parents and ensure they are present
        let parent_set: PreHashSet<BlockId> = header.content.parents.iter().copied().collect();
        for parent_thread in 0u8..self.config.thread_count {
            let parent_hash = header.content.parents[parent_thread as usize];
            match read_shared_state.block_statuses.get(&parent_hash) {
                Some(BlockStatus::Discarded { reason, .. }) => {
                    // parent is discarded
                    return Ok(HeaderCheckOutcome::Discard(match reason {
                        DiscardReason::Invalid(invalid_reason) => DiscardReason::Invalid(format!(
                            "discarded because a parent was discarded for the following reason: {}",
                            invalid_reason
                        )),
                        r => r.clone(),
                    }));
                }
                Some(BlockStatus::Active {
                    a_block: parent, ..
                }) => {
                    // parent is active

                    // check that the parent is from an earlier slot in the right thread
                    if parent.slot.thread != parent_thread || parent.slot >= header.content.slot {
                        return Ok(HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                            format!(
                                "Bad parent {} in thread:{} or slot:{} for {}.",
                                parent_hash, parent_thread, parent.slot, header.content.slot
                            ),
                        )));
                    }

                    // inherit parent incompatibilities
                    // and ensure parents are mutually compatible
                    if let Some(p_incomp) = read_shared_state.gi_head.get(&parent_hash) {
                        if !p_incomp.is_disjoint(&parent_set) {
                            return Ok(HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                                "Parent not mutually compatible".to_string(),
                            )));
                        }
                        incomp.extend(p_incomp);
                    }

                    parents.push((parent_hash, parent.slot.period));
                }
                _ => {
                    // parent is missing or queued
                    if read_shared_state.genesis_hashes.contains(&parent_hash) {
                        // forbid depending on discarded genesis block
                        return Ok(HeaderCheckOutcome::Discard(DiscardReason::Stale));
                    }
                    missing_deps.insert(parent_hash);
                }
            }
        }
        if !missing_deps.is_empty() {
            return Ok(HeaderCheckOutcome::WaitForDependencies(missing_deps));
        }
        let inherited_incomp_count = incomp.len();

        // check the topological consistency of the parents
        {
            let mut gp_max_slots = vec![0u64; self.config.thread_count as usize];
            for parent_i in 0..self.config.thread_count {
                let (parent_h, parent_period) = parents[parent_i as usize];
                let parent = match read_shared_state.block_statuses.get(&parent_h) {
                    Some(BlockStatus::Active {
                        a_block,
                        storage: _,
                    }) => a_block,
                    _ => {
                        return Err(ConsensusError::ContainerInconsistency(format!(
                            "inconsistency inside block statuses searching parent {} of block {}",
                            parent_h, block_id
                        )))
                    }
                };
                if parent_period < gp_max_slots[parent_i as usize] {
                    // a parent is earlier than a block known by another parent in that thread
                    return Ok(HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                        "a parent is earlier than a block known by another parent in that thread"
                            .to_string(),
                    )));
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
                    match read_shared_state.block_statuses.get(&gp_h) {
                        // this grandpa is discarded
                        Some(BlockStatus::Discarded { reason, .. }) => {
                            return Ok(HeaderCheckOutcome::Discard(reason.clone()));
                        }
                        // this grandpa is active
                        Some(BlockStatus::Active { a_block: gp, .. }) => {
                            if gp.slot.period > gp_max_slots[gp_i as usize] {
                                if gp_i < parent_i {
                                    return Ok(HeaderCheckOutcome::Discard(
                                        DiscardReason::Invalid(
                                            "grandpa error: gp_i < parent_i".to_string(),
                                        ),
                                    ));
                                }
                                gp_max_slots[gp_i as usize] = gp.slot.period;
                            }
                        }
                        // this grandpa is missing, assume stale
                        _ => return Ok(HeaderCheckOutcome::Discard(DiscardReason::Stale)),
                    }
                }
            }
        }

        // get parent in own thread
        let parent_in_own_thread = match read_shared_state
            .block_statuses
            .get(&parents[header.content.slot.thread as usize].0)
        {
            Some(BlockStatus::Active {
                a_block,
                storage: _,
            }) => Some(a_block),
            _ => None,
        }
        .ok_or_else(|| {
            ConsensusError::ContainerInconsistency(format!(
                "inconsistency inside block statuses searching parent {} in own thread of block {}",
                parents[header.content.slot.thread as usize].0, block_id
            ))
        })?;

        // check endorsements
        match self.check_endorsements(header)? {
            EndorsementsCheckOutcome::Proceed => {}
            EndorsementsCheckOutcome::Discard(reason) => {
                return Ok(HeaderCheckOutcome::Discard(reason))
            }
            EndorsementsCheckOutcome::WaitForSlot => return Ok(HeaderCheckOutcome::WaitForSlot),
        }

        // thread incompatibility test
        parent_in_own_thread.children[header.content.slot.thread as usize]
            .keys()
            .filter(|&sibling_h| sibling_h != block_id)
            .try_for_each(|&sibling_h| {
                incomp.extend(self.get_active_block_and_descendants(&sibling_h)?);
                Result::<(), ConsensusError>::Ok(())
            })?;

        // grandpa incompatibility test
        for tau in (0u8..self.config.thread_count).filter(|&t| t != header.content.slot.thread) {
            // for each parent in a different thread tau
            // traverse parent's descendants in tau
            let mut to_explore = vec![(0usize, header.content.parents[tau as usize])];
            while let Some((cur_gen, cur_h)) = to_explore.pop() {
                let cur_b = match read_shared_state.block_statuses.get(&cur_h) {
                    Some(BlockStatus::Active { a_block, storage: _ }) => Some(a_block),
                    _ => None,
                }.ok_or_else(|| ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses searching {} while checking grandpa incompatibility of block {}",cur_h,  block_id)))?;

                // traverse but do not check up to generation 1
                if cur_gen <= 1 {
                    to_explore.extend(
                        cur_b.children[tau as usize]
                            .keys()
                            .map(|&c_h| (cur_gen + 1, c_h)),
                    );
                    continue;
                }

                let parent_id = {
                    self.storage
                        .read_blocks()
                        .get(&cur_b.block_id)
                        .ok_or_else(|| {
                            ConsensusError::MissingBlock(format!(
                                "missing block in grandpa incomp test: {}",
                                cur_b.block_id
                            ))
                        })?
                        .content
                        .header
                        .content
                        .parents[header.content.slot.thread as usize]
                };

                // check if the parent in tauB has a strictly lower period number than B's parent in tauB
                // note: cur_b cannot be genesis at gen > 1
                let parent_period = match read_shared_state.block_statuses.get(&parent_id) {
                    Some(BlockStatus::Active { a_block, storage: _ }) => Some(a_block),
                    _ => None,
                }.ok_or_else(||
                    ConsensusError::ContainerInconsistency(
                        format!("inconsistency inside block statuses searching {} check if the parent in tauB has a strictly lower period number than B's parent in tauB while checking grandpa incompatibility of block {}",
                        parent_id,
                        block_id)
                    ))?.slot.period;
                if parent_period < parent_in_own_thread.slot.period {
                    // GPI detected
                    incomp.extend(self.get_active_block_and_descendants(&cur_h)?);
                } // otherwise, cur_b and its descendants cannot be GPI with the block: don't traverse
            }
        }

        // check if the block is incompatible with a parent
        if !incomp.is_disjoint(&parents.iter().map(|(h, _p)| *h).collect()) {
            return Ok(HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                "Block incompatible with a parent".to_string(),
            )));
        }

        // check if the block is incompatible with a final block
        if !incomp.is_disjoint(
            &read_shared_state
                .active_index
                .iter()
                .filter_map(|h| {
                    if let Some(BlockStatus::Active { a_block: a, .. }) =
                        read_shared_state.block_statuses.get(h)
                    {
                        if a.is_final {
                            return Some(*h);
                        }
                    }
                    None
                })
                .collect(),
        ) {
            return Ok(HeaderCheckOutcome::Discard(DiscardReason::Stale));
        }
        massa_trace!("consensus.block_graph.check_header.ok", {
            "block_id": block_id
        });

        Ok(HeaderCheckOutcome::Proceed {
            parents_hash_period: parents,
            incompatibilities: incomp,
            inherited_incompatibilities_count: inherited_incomp_count,
            fitness: header.get_fitness(),
        })
    }

    /// check endorsements:
    /// * endorser was selected for that (slot, index)
    /// * endorsed slot is `parent_in_own_thread` slot
    pub fn check_endorsements(
        &self,
        header: &SecuredHeader,
    ) -> Result<EndorsementsCheckOutcome, ConsensusError> {
        // check endorsements
        let endorsement_draws = match self
            .channels
            .selector_controller
            .get_selection(header.content.slot)
        {
            Ok(sel) => sel.endorsements,
            Err(_) => return Ok(EndorsementsCheckOutcome::WaitForSlot),
        };
        for endorsement in header.content.endorsements.iter() {
            // check that the draw is correct
            if endorsement.content_creator_address
                != endorsement_draws[endorsement.content.index as usize]
            {
                return Ok(EndorsementsCheckOutcome::Discard(DiscardReason::Invalid(
                    format!(
                        "endorser draw mismatch for header in slot: {}",
                        header.content.slot
                    ),
                )));
            }

            // note that the following aspects are checked in protocol
            // * signature
            // * index reuse
            // * slot matching the block's
            // * the endorsed block is the containing block's parent
        }

        Ok(EndorsementsCheckOutcome::Proceed)
    }
}
