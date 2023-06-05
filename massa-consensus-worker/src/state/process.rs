use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    mem,
};

use massa_consensus_exports::{
    block_status::{BlockStatus, DiscardReason, HeaderOrBlock},
    error::ConsensusError,
};
use massa_logging::massa_trace;
use massa_models::{
    active_block::ActiveBlock,
    address::Address,
    block_header::SecuredHeader,
    block_id::BlockId,
    clique::Clique,
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
};
use massa_signature::PublicKey;
use massa_storage::Storage;
use massa_time::MassaTime;
use tracing::log::{debug, info};

use crate::state::clique_computation::compute_max_cliques;

use super::ConsensusState;

/// All informations necessary to add a block to the graph
pub(crate) struct BlockInfos {
    /// The block creator
    pub creator: PublicKey,
    /// The slot of the block
    pub slot: Slot,
    /// The list of the parents of the block (block_id, period) (one block per thread)
    pub parents_hash_period: Vec<(BlockId, u64)>,
    /// The list of the blocks that are incompatible with this block
    pub incompatibilities: PreHashSet<BlockId>,
    /// Number of incompatibilities this block inherit from his parents
    pub inherited_incompatibilities_count: usize,
    /// THe storage can the block himself and his operations and endorsements
    pub storage: Storage,
    /// The fitness of the block
    pub fitness: u64,
}

impl ConsensusState {
    /// Acknowledge a set of items recursively and process them
    ///
    /// # Arguments:
    /// * `to_ack`: the set of items to acknowledge and process
    /// * `current_slot`: the current slot when this function is called
    ///
    /// # Returns:
    /// Success or error if an error happened during the processing of items
    pub fn rec_process(
        &mut self,
        mut to_ack: BTreeSet<(Slot, BlockId)>,
        current_slot: Option<Slot>,
    ) -> Result<(), ConsensusError> {
        // order processing by (slot, hash)
        while let Some((_slot, hash)) = to_ack.pop_first() {
            // When a slot and a block ID is processed through the `process` function, it is possible that it causes others blocks
            // to need processing as well. In this case the `process` function will return them and they will be added to
            // the `to_ack` vector to be processed in the future.
            to_ack.extend(self.process(hash, current_slot)?)
        }
        Ok(())
    }

    /// Acknowledge a single item, return a set of items to re-ack
    ///
    /// # Arguments:
    /// * `block_id`: the id of the block to acknowledge
    /// * `current_slot`: the current slot when this function is called
    ///
    /// # Returns:
    /// A list of items to re-ack and process or an error if the process of an item failed
    fn process(
        &mut self,
        block_id: BlockId,
        current_slot: Option<Slot>,
    ) -> Result<BTreeSet<(Slot, BlockId)>, ConsensusError> {
        // list items to reprocess
        let mut reprocess = BTreeSet::new();

        massa_trace!("consensus.block_graph.process", { "block_id": block_id });
        // control all the waiting states and try to get a valid block
        let valid_block_infos = match self.block_statuses.get(&block_id) {
            None => return Ok(BTreeSet::new()), // disappeared before being processed: do nothing

            // discarded: do nothing
            Some(BlockStatus::Discarded { .. }) => {
                massa_trace!("consensus.block_graph.process.discarded", {
                    "block_id": block_id
                });
                return Ok(BTreeSet::new());
            }

            // already active: do nothing
            Some(BlockStatus::Active { .. }) => {
                massa_trace!("consensus.block_graph.process.active", {
                    "block_id": block_id
                });
                return Ok(BTreeSet::new());
            }

            // incoming header
            Some(BlockStatus::Incoming(HeaderOrBlock::Header(_))) => {
                massa_trace!("consensus.block_graph.process.incoming_header", {
                    "block_id": block_id
                });
                // remove header
                let header = if let Some(BlockStatus::Incoming(HeaderOrBlock::Header(header))) =
                    self.block_statuses.remove(&block_id)
                {
                    self.incoming_index.remove(&block_id);
                    header
                } else {
                    return Err(ConsensusError::ContainerInconsistency(format!(
                        "inconsistency inside block statuses removing incoming header {}",
                        block_id
                    )));
                };
                self.check_block_header_and_store(block_id, header, current_slot)?;
                return Ok(BTreeSet::new());
            }

            // incoming block
            Some(BlockStatus::Incoming(HeaderOrBlock::Block { id: block_id, .. })) => {
                let block_id = *block_id;
                massa_trace!("consensus.block_graph.process.incoming_block", {
                    "block_id": block_id
                });
                let (slot, storage) =
                    if let Some(BlockStatus::Incoming(HeaderOrBlock::Block {
                        slot, storage, ..
                    })) = self.block_statuses.remove(&block_id)
                    {
                        self.incoming_index.remove(&block_id);
                        (slot, storage)
                    } else {
                        return Err(ConsensusError::ContainerInconsistency(format!(
                            "inconsistency inside block statuses removing incoming block {}",
                            block_id
                        )));
                    };
                let stored_block = storage
                    .read_blocks()
                    .get(&block_id)
                    .cloned()
                    .expect("incoming block not found in storage");
                match self.check_block_and_store(
                    block_id,
                    slot,
                    storage,
                    stored_block,
                    current_slot,
                )? {
                    Some(block_infos) => block_infos,
                    None => return Ok(BTreeSet::new()),
                }
            }

            Some(BlockStatus::WaitingForSlot(header_or_block)) => {
                massa_trace!("consensus.block_graph.process.waiting_for_slot", {
                    "block_id": block_id
                });
                let slot = header_or_block.get_slot();
                if Some(slot) > current_slot {
                    massa_trace!(
                        "consensus.block_graph.process.waiting_for_slot.in_the_future",
                        { "block_id": block_id }
                    );
                    // in the future: ignore
                    return Ok(BTreeSet::new());
                }
                // send back as incoming and ask for reprocess
                if let Some(BlockStatus::WaitingForSlot(header_or_block)) =
                    self.block_statuses.remove(&block_id)
                {
                    self.waiting_for_slot_index.remove(&block_id);
                    self.block_statuses
                        .insert(block_id, BlockStatus::Incoming(header_or_block));
                    self.incoming_index.insert(block_id);
                    reprocess.insert((slot, block_id));
                    massa_trace!(
                        "consensus.block_graph.process.waiting_for_slot.reprocess",
                        { "block_id": block_id }
                    );
                    return Ok(reprocess);
                } else {
                    return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses removing waiting for slot block or header {}", block_id)));
                };
            }

            Some(BlockStatus::WaitingForDependencies {
                unsatisfied_dependencies,
                ..
            }) => {
                massa_trace!("consensus.block_graph.process.waiting_for_dependencies", {
                    "block_id": block_id
                });
                if !unsatisfied_dependencies.is_empty() {
                    // still has unsatisfied dependencies: ignore
                    return Ok(BTreeSet::new());
                }
                // send back as incoming and ask for reprocess
                if let Some(BlockStatus::WaitingForDependencies {
                    header_or_block, ..
                }) = self.block_statuses.remove(&block_id)
                {
                    self.waiting_for_dependencies_index.remove(&block_id);
                    reprocess.insert((header_or_block.get_slot(), block_id));
                    self.block_statuses
                        .insert(block_id, BlockStatus::Incoming(header_or_block));
                    self.incoming_index.insert(block_id);
                    massa_trace!(
                        "consensus.block_graph.process.waiting_for_dependencies.reprocess",
                        { "block_id": block_id }
                    );
                    return Ok(reprocess);
                } else {
                    return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses removing waiting for slot header or block {}", block_id)));
                }
            }
        };

        // add block to graph
        self.add_block_to_graph(
            block_id,
            valid_block_infos.parents_hash_period,
            valid_block_infos.creator,
            valid_block_infos.slot,
            valid_block_infos.incompatibilities,
            valid_block_infos.inherited_incompatibilities_count,
            valid_block_infos.fitness,
            valid_block_infos.storage,
        )?;

        // if the block was added, update linked dependencies and mark satisfied ones for recheck
        if let Some(BlockStatus::Active { storage, .. }) = self.block_statuses.get(&block_id) {
            massa_trace!("consensus.block_graph.process.is_active", {
                "block_id": block_id
            });
            self.to_propagate.insert(block_id, storage.clone());
            for itm_block_id in self.waiting_for_dependencies_index.iter() {
                if let Some(BlockStatus::WaitingForDependencies {
                    header_or_block,
                    unsatisfied_dependencies,
                    ..
                }) = self.block_statuses.get_mut(itm_block_id)
                {
                    if unsatisfied_dependencies.remove(&block_id) {
                        // a dependency was satisfied: retry
                        reprocess.insert((header_or_block.get_slot(), *itm_block_id));
                    }
                }
            }
        }

        Ok(reprocess)
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

    /// Add a block to the graph and update the cliques, the graph dependencies and incompatibilities
    ///
    /// # Arguments:
    /// * `add_block_id`: Block id of the block to add
    /// * `parents_hash_period`: Ids and periods of the parents of the block to add
    /// * `add_block_creator`: Creator of the block to add
    /// * `add_block_slot`: Slot of the block to add
    /// * `incomp`: Block ids of the blocks incompatible with the block to add
    /// * `fitness`: Fitness of the block to add
    /// * `storage`: Storage containing all the data of the block to add
    ///
    /// # Returns:
    /// Success or error if any steps failed
    #[allow(clippy::too_many_arguments)]
    fn add_block_to_graph(
        &mut self,
        add_block_id: BlockId,
        parents_hash_period: Vec<(BlockId, u64)>,
        add_block_creator: PublicKey,
        add_block_slot: Slot,
        incomp: PreHashSet<BlockId>,
        inherited_incomp_count: usize,
        fitness: u64,
        mut storage: Storage,
    ) -> Result<(), ConsensusError> {
        massa_trace!("consensus.block_graph.add_block_to_graph", {
            "block_id": add_block_id
        });

        // Ensure block parents are claimed by the block's storage.
        // Note that operations and endorsements should already be there (claimed in Protocol).
        storage.claim_block_refs(&parents_hash_period.iter().map(|(p_id, _)| *p_id).collect());

        // add block to status structure
        self.block_statuses.insert(
            add_block_id,
            BlockStatus::Active {
                a_block: Box::new(ActiveBlock {
                    creator_address: Address::from_public_key(&add_block_creator),
                    parents: parents_hash_period.clone(),
                    descendants: PreHashSet::<BlockId>::default(),
                    block_id: add_block_id,
                    children: vec![Default::default(); self.config.thread_count as usize],
                    is_final: false,
                    slot: add_block_slot,
                    fitness,
                }),
                storage,
            },
        );
        self.active_index.insert(add_block_id);

        // add as child to parents
        // add as descendant to ancestors. Note: descendants are never removed.
        self.insert_parents_descendants(
            add_block_id,
            add_block_slot,
            parents_hash_period.iter().map(|(p_id, _)| *p_id).collect(),
        );

        // add incompatibilities to gi_head
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.add_incompatibilities",
            {}
        );
        for incomp_h in incomp.iter() {
            self.gi_head
                .get_mut(incomp_h)
                .ok_or_else(|| {
                    ConsensusError::MissingBlock(format!(
                        "missing block when adding incomp to gi_head: {}",
                        incomp_h
                    ))
                })?
                .insert(add_block_id);
        }
        self.gi_head.insert(add_block_id, incomp.clone());

        // max cliques update
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.max_cliques_update",
            {}
        );
        if incomp.len() == inherited_incomp_count {
            // clique optimization routine:
            //   the block only has incompatibilities inherited from its parents
            //   therefore it is not forking and can simply be added to the cliques it is compatible with
            self.max_cliques
                .iter_mut()
                .filter(|c| incomp.is_disjoint(&c.block_ids))
                .for_each(|c| {
                    c.block_ids.insert(add_block_id);
                });
        } else {
            // fully recompute max cliques
            massa_trace!(
                "consensus.block_graph.add_block_to_graph.clique_full_computing",
                { "hash": add_block_id }
            );
            let before = self.max_cliques.len();
            self.max_cliques = compute_max_cliques(&self.gi_head)
                .into_iter()
                .map(|c| Clique {
                    block_ids: c,
                    fitness: 0,
                    is_blockclique: false,
                })
                .collect();
            let after = self.max_cliques.len();
            if before != after {
                massa_trace!(
                    "consensus.block_graph.add_block_to_graph.clique_full_computing more than one clique",
                    { "cliques": self.max_cliques, "gi_head": self.gi_head }
                );
                // gi_head
                debug!(
                    "clique number went from {} to {} after adding {}",
                    before, after, add_block_id
                );
            }
        }

        // compute clique fitnesses and find blockclique
        massa_trace!("consensus.block_graph.add_block_to_graph.compute_clique_fitnesses_and_find_blockclique", {});
        // note: clique_fitnesses is pair (fitness, -hash_sum) where the second parameter is negative for sorting
        let position_blockclique = self.compute_fitness_find_blockclique(&add_block_id)?;

        // update best parents
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.update_best_parents",
            {}
        );
        {
            let blockclique = &self.max_cliques[position_blockclique];

            // init best parents as latest_final_blocks_periods
            self.best_parents = self.latest_final_blocks_periods.clone();
            // for each blockclique block, set it as best_parent in its own thread
            // if its period is higher than the current best_parent in that thread
            for block_h in blockclique.block_ids.iter() {
                let b_slot = match self.block_statuses.get(block_h) {
                    Some(BlockStatus::Active { a_block, storage: _ }) => a_block.slot,
                    _ => return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses updating best parents while adding {} - missing {}", add_block_id, block_h))),
                };
                if b_slot.period > self.best_parents[b_slot.thread as usize].1 {
                    self.best_parents[b_slot.thread as usize] = (*block_h, b_slot.period);
                }
            }
        }

        // list stale blocks
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.list_stale_blocks",
            {}
        );
        let fitness_threshold = self.max_cliques[position_blockclique]
            .fitness
            .saturating_sub(self.config.delta_f0);
        let stale_blocks = self.list_stale_blocks(fitness_threshold);
        self.max_cliques.retain(|c| c.fitness >= fitness_threshold);
        // mark stale blocks
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.mark_stale_blocks",
            {}
        );
        for stale_block_hash in stale_blocks.into_iter() {
            self.remove_block(&add_block_id, &stale_block_hash)?;
        }

        // list final blocks
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.list_final_blocks",
            {}
        );
        let final_blocks = self.list_final_blocks()?;

        // mark final blocks and update latest_final_blocks_periods
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.mark_final_blocks",
            {}
        );
        self.mark_final_blocks(&add_block_id, final_blocks)?;

        massa_trace!("consensus.block_graph.add_block_to_graph.end", {});
        Ok(())
    }

    /// Note an attack attempt if the discard reason indicates one.
    pub fn maybe_note_attack_attempt(&mut self, reason: &DiscardReason, hash: &BlockId) {
        massa_trace!("consensus.block_graph.maybe_note_attack_attempt", {"hash": hash, "reason": reason});
        // If invalid, note the attack attempt.
        if let DiscardReason::Invalid(reason) = reason {
            info!(
                "consensus.block_graph.maybe_note_attack_attempt DiscardReason::Invalid:{}",
                reason
            );
            self.attack_attempts.push(*hash);
        }
    }

    /// Notify execution about blockclique changes and finalized blocks.
    ///
    /// # Arguments:
    /// * `finalized_blocks`: Block that became final and need to be send to execution
    fn notify_execution(&mut self, finalized_blocks: HashMap<Slot, BlockId>) {
        // List new block storage instances that Execution doesn't know about.
        // That's blocks that have not been sent to execution before, ie. in the previous blockclique).
        let mut new_blocks_storage: PreHashMap<BlockId, Storage> = finalized_blocks
            .iter()
            .filter_map(|(_slot, b_id)| {
                if self.prev_blockclique.contains_key(b_id) {
                    // was previously sent as a blockclique element
                    return None;
                }
                let storage = match self.block_statuses.get(b_id) {
                    Some(BlockStatus::Active {
                        a_block: _,
                        storage,
                    }) => storage,
                    _ => panic!("final block not found in active blocks"),
                };
                Some((*b_id, storage.clone()))
            })
            .collect();

        // Get new blockclique block list with slots.
        let mut blockclique_changed = false;
        let new_blockclique: PreHashMap<BlockId, Slot> = self
            .get_blockclique()
            .iter()
            .map(|b_id| {
                if let Some(slot) = self.prev_blockclique.remove(b_id) {
                    // The block was already sent in the previous blockclique:
                    // the slot can be gathered from there without locking Storage.
                    // Note: the block is removed from self.prev_blockclique.
                    (*b_id, slot)
                } else {
                    // The block was not present in the previous blockclique:
                    // the blockclique has changed => get the block's slot by querying Storage.
                    blockclique_changed = true;
                    let (slot, storage) = match self.block_statuses.get(b_id) {
                        Some(BlockStatus::Active { a_block, storage }) => (a_block.slot, storage),
                        _ => panic!("blockclique block not found in active blocks"),
                    };
                    new_blocks_storage.insert(*b_id, storage.clone());
                    (*b_id, slot)
                }
            })
            .collect();
        if !self.prev_blockclique.is_empty() {
            // All elements present in the new blockclique have been removed from `prev_blockclique` above.
            // If `prev_blockclique` is not empty here, it means that it contained elements that are not in the new blockclique anymore.
            // In that case, we mark the blockclique as having changed.
            blockclique_changed = true;
        }
        // Overwrite previous blockclique.
        // Should still be done even if unchanged because elements were removed from it above.
        self.prev_blockclique = new_blockclique.clone();

        if finalized_blocks.is_empty() && !blockclique_changed {
            // There are no changes (neither block finalizations not blockclique changes) to send to execution.
            return;
        }

        // Notify execution of block finalizations and blockclique changes
        self.channels
            .execution_controller
            .update_blockclique_status(
                finalized_blocks,
                if blockclique_changed {
                    Some(new_blockclique.into_iter().map(|(k, v)| (v, k)).collect())
                } else {
                    None
                },
                new_blocks_storage,
            );
    }

    /// call me if the block database changed
    /// Processing of final blocks, pruning.
    ///
    /// 1. propagate blocks
    /// 2. Notify of attack attempts
    /// 3. get new final blocks
    /// 4. get blockclique
    /// 5. notify Execution
    /// 6. Process new final blocks
    /// 7. Notify pool of new final ops
    /// 8. Notify PoS of final blocks
    /// 9. notify protocol of block wish list
    /// 10. note new latest final periods (prune graph if changed)
    /// 11. add stale blocks to stats
    pub fn block_db_changed(&mut self) -> Result<(), ConsensusError> {
        let final_block_slots = {
            massa_trace!("consensus.consensus_worker.block_db_changed", {});

            // Propagate new blocks
            for (block_id, storage) in mem::take(&mut self.to_propagate).into_iter() {
                massa_trace!("consensus.consensus_worker.block_db_changed.integrated", {
                    "block_id": block_id
                });
                self.channels
                    .protocol_controller
                    .integrated_block(block_id, storage)?;
            }

            // Notify protocol of attack attempts.
            for hash in mem::take(&mut self.attack_attempts).into_iter() {
                self.channels
                    .protocol_controller
                    .notify_block_attack(hash)?;
                massa_trace!("consensus.consensus_worker.block_db_changed.attack", {
                    "hash": hash
                });
            }

            // manage finalized blocks
            let timestamp = MassaTime::now()?;
            let finalized_blocks = mem::take(&mut self.new_final_blocks);
            let mut final_block_slots = HashMap::with_capacity(finalized_blocks.len());
            let mut final_block_stats = VecDeque::with_capacity(finalized_blocks.len());
            for b_id in finalized_blocks {
                if let Some(BlockStatus::Active {
                    a_block,
                    storage: _,
                }) = self.block_statuses.get(&b_id)
                {
                    // add to final blocks to notify execution
                    final_block_slots.insert(a_block.slot, b_id);

                    // add to stats
                    let block_is_from_protocol = self
                        .protocol_blocks
                        .iter()
                        .any(|(_, block_id)| block_id == &b_id);
                    final_block_stats.push_back((
                        timestamp,
                        a_block.creator_address,
                        block_is_from_protocol,
                    ));
                }
            }
            self.final_block_stats.extend(final_block_stats);

            // add stale blocks to stats
            let new_stale_block_ids_creators_slots = mem::take(&mut self.new_stale_blocks);
            let timestamp = MassaTime::now()?;
            for (_b_id, (_b_creator, _b_slot)) in new_stale_block_ids_creators_slots.into_iter() {
                self.stale_block_stats.push_back(timestamp);
            }
            final_block_slots
        };

        // notify execution
        self.notify_execution(final_block_slots);

        // notify protocol of block wishlist
        let new_wishlist = self.get_block_wishlist()?;
        let new_blocks: PreHashMap<BlockId, Option<SecuredHeader>> = new_wishlist
            .iter()
            .filter_map(|(id, header)| {
                if !self.wishlist.contains_key(id) {
                    Some((*id, header.clone()))
                } else {
                    None
                }
            })
            .collect();
        let remove_blocks: PreHashSet<BlockId> = self
            .wishlist
            .iter()
            .filter_map(|(id, _)| {
                if !new_wishlist.contains_key(id) {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();
        if !new_blocks.is_empty() || !remove_blocks.is_empty() {
            massa_trace!("consensus.consensus_worker.block_db_changed.send_wishlist_delta", { "new": new_wishlist, "remove": remove_blocks });
            self.channels
                .protocol_controller
                .send_wishlist_delta(new_blocks, remove_blocks)?;
            self.wishlist = new_wishlist;
        }

        // note new latest final periods
        let latest_final_periods: Vec<u64> = self
            .latest_final_blocks_periods
            .iter()
            .map(|(_block_id, period)| *period)
            .collect();
        // if changed...
        if self.save_final_periods != latest_final_periods {
            // signal new last final periods to pool
            self.channels
                .pool_controller
                .notify_final_cs_periods(&latest_final_periods);
            // update final periods
            self.save_final_periods = latest_final_periods;
        }

        Ok(())
    }
}
