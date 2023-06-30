use std::collections::VecDeque;

use massa_consensus_exports::{
    block_status::{BlockStatus, DiscardReason},
    error::ConsensusError,
};
use massa_logging::massa_trace;
use massa_models::{block_id::BlockId, clique::Clique, prehash::PreHashSet, slot::Slot};

use super::ConsensusState;

impl ConsensusState {
    pub fn insert_parents_descendants(
        &mut self,
        add_block_id: BlockId,
        add_block_slot: Slot,
        parents_hash: Vec<BlockId>,
    ) {
        // add as child to parents
        for parent_h in parents_hash.iter() {
            if let Some(BlockStatus::Active {
                a_block: a_parent, ..
            }) = self.blocks_state.get_mut(parent_h)
            {
                a_parent.children[add_block_slot.thread as usize]
                    .insert(add_block_id, add_block_slot.period);
            }
        }

        // add as descendant to ancestors. Note: descendants are never removed.
        let mut ancestors: VecDeque<BlockId> = parents_hash.iter().copied().collect();
        let mut visited = PreHashSet::<BlockId>::default();
        while let Some(ancestor_h) = ancestors.pop_back() {
            if !visited.insert(ancestor_h) {
                continue;
            }
            if let Some(BlockStatus::Active { a_block: ab, .. }) =
                self.blocks_state.get_mut(&ancestor_h)
            {
                ab.descendants.insert(add_block_id);
                for (ancestor_parent_h, _) in ab.parents.iter() {
                    ancestors.push_front(*ancestor_parent_h);
                }
            }
        }
    }

    pub fn compute_fitness_find_blockclique(
        &mut self,
        add_block_id: &BlockId,
    ) -> Result<usize, ConsensusError> {
        let mut blockclique_i = 0usize;
        let mut max_clique_fitness = (0u64, num::BigInt::default());
        for (clique_i, clique) in self.max_cliques.iter_mut().enumerate() {
            clique.fitness = 0;
            clique.is_blockclique = false;
            let mut sum_hash = num::BigInt::default();
            for block_h in clique.block_ids.iter() {
                let fitness = match self.blocks_state.get(block_h) {
                    Some(BlockStatus::Active { a_block, storage: _ }) => a_block.fitness,
                    _ => return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses computing fitness while adding {} - missing {}", add_block_id, block_h))),
                };
                clique.fitness = clique
                    .fitness
                    .checked_add(fitness)
                    .ok_or(ConsensusError::FitnessOverflow)?;
                sum_hash -= num::BigInt::from_bytes_be(num::bigint::Sign::Plus, block_h.to_bytes());
            }
            let cur_fit = (clique.fitness, sum_hash);
            if cur_fit > max_clique_fitness {
                blockclique_i = clique_i;
                max_clique_fitness = cur_fit;
            }
        }
        self.max_cliques[blockclique_i].is_blockclique = true;
        Ok(blockclique_i)
    }

    pub fn list_stale_blocks(&self, fitness_threshold: u64) -> PreHashSet<BlockId> {
        // iterate from largest to smallest to minimize reallocations
        let mut indices: Vec<usize> = (0..self.max_cliques.len()).collect();
        indices.sort_unstable_by_key(|&i| std::cmp::Reverse(self.max_cliques[i].block_ids.len()));
        let mut high_set = PreHashSet::<BlockId>::default();
        let mut low_set = PreHashSet::<BlockId>::default();
        for clique_i in indices.into_iter() {
            if self.max_cliques[clique_i].fitness >= fitness_threshold {
                high_set.extend(&self.max_cliques[clique_i].block_ids);
            } else {
                low_set.extend(&self.max_cliques[clique_i].block_ids);
            }
        }
        &low_set - &high_set
    }

    pub fn remove_block(&mut self, add_block_id: &BlockId, block_id: &BlockId) {
        let sequence_number = self.blocks_state.sequence_counter();
        self.blocks_state.transition_map(block_id, |block_status, block_statuses| {
        if let Some(BlockStatus::Active {
            a_block: active_block,
            storage: _storage,
        }) = block_status
        {
            if active_block.is_final {
               panic!("inconsistency inside block statuses removing stale blocks adding {} - block {} was already final", add_block_id, block_id);
            }

            // remove from gi_head
            if let Some(other_incomps) = self.gi_head.remove(block_id) {
                for other_incomp in other_incomps.into_iter() {
                    if let Some(other_incomp_lst) = self.gi_head.get_mut(&other_incomp) {
                        other_incomp_lst.remove(block_id);
                    }
                }
            }

            // remove from cliques
            let stale_block_fitness = active_block.fitness;
            self.max_cliques.iter_mut().for_each(|c| {
                if c.block_ids.remove(block_id) {
                    c.fitness -= stale_block_fitness;
                }
            });
            self.max_cliques.retain(|c| !c.block_ids.is_empty()); // remove empty cliques
            if self.max_cliques.is_empty() {
                // make sure at least one clique remains
                self.max_cliques = vec![Clique {
                    block_ids: PreHashSet::<BlockId>::default(),
                    fitness: 0,
                    is_blockclique: true,
                }];
            }

            // remove from parent's children
            for (parent_h, _parent_period) in active_block.parents.iter() {
                if let Some(BlockStatus::Active {
                    a_block: parent_active_block,
                    ..
                }) = block_statuses.get_mut(parent_h)
                {
                    parent_active_block.children[active_block.slot.thread as usize]
                        .remove(block_id);
                }
            }

            massa_trace!("consensus.block_graph.add_block_to_graph.stale", {
                "hash": block_id
            });

            // mark as stale
            self.new_stale_blocks
                .insert(*block_id, (active_block.creator_address, active_block.slot));
            Some(
                BlockStatus::Discarded {
                    slot: active_block.slot,
                    creator: active_block.creator_address,
                    parents: active_block.parents.iter().map(|(h, _)| *h).collect(),
                    reason: DiscardReason::Stale,
                    sequence_number,
                }
            )
        } else {
            panic!("inconsistency inside block statuses removing stale blocks adding {} - block {} is missing", add_block_id, block_id);
        }
    });
    }

    pub fn list_final_blocks(&self) -> Result<PreHashSet<BlockId>, ConsensusError> {
        // short-circuiting intersection of cliques from smallest to largest
        let mut indices: Vec<usize> = (0..self.max_cliques.len()).collect();
        indices.sort_unstable_by_key(|&i| self.max_cliques[i].block_ids.len());
        let mut indices_iter = indices.iter();
        let mut final_candidates = self.max_cliques
            [*indices_iter.next().expect("expected at least one clique")]
        .block_ids
        .clone();
        for i in indices_iter {
            final_candidates.retain(|v| self.max_cliques[*i].block_ids.contains(v));
            if final_candidates.is_empty() {
                break;
            }
        }

        // restrict search to cliques with high enough fitness, sort cliques by fitness (highest to lowest)
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.list_final_blocks.restrict",
            {}
        );
        indices.retain(|&i| self.max_cliques[i].fitness > self.config.delta_f0);
        indices.sort_unstable_by_key(|&i| std::cmp::Reverse(self.max_cliques[i].fitness));

        let mut final_blocks = PreHashSet::<BlockId>::default();
        for clique_i in indices.into_iter() {
            massa_trace!(
                "consensus.block_graph.add_block_to_graph.list_final_blocks.loop",
                { "clique_i": clique_i }
            );
            // check in cliques from highest to lowest fitness
            if final_candidates.is_empty() {
                // no more final candidates
                break;
            }
            let clique = &self.max_cliques[clique_i];

            // compute the total fitness of all the descendants of the candidate within the clique
            let loc_candidates = final_candidates.clone();
            for candidate_h in loc_candidates.into_iter() {
                let descendants = match self.blocks_state.get(&candidate_h) {
                    Some(BlockStatus::Active {
                        a_block,
                        storage: _,
                    }) => &a_block.descendants,
                    _ => {
                        return Err(ConsensusError::MissingBlock(format!(
                            "missing block when computing total fitness of descendants: {}",
                            candidate_h
                        )))
                    }
                };
                let desc_fit: u64 = descendants
                    .intersection(&clique.block_ids)
                    .map(|h| {
                        if let Some(BlockStatus::Active { a_block: ab, .. }) =
                            self.blocks_state.get(h)
                        {
                            return ab.fitness;
                        }
                        0
                    })
                    .sum();
                if desc_fit > self.config.delta_f0 {
                    // candidate is final
                    final_candidates.remove(&candidate_h);
                    final_blocks.insert(candidate_h);
                }
            }
        }
        Ok(final_blocks)
    }

    /// get the clique of higher fitness
    pub fn get_blockclique(&self) -> PreHashSet<BlockId> {
        self.max_cliques
            .iter()
            .find(|c| c.is_blockclique)
            .expect("blockclique missing")
            .block_ids
            .clone()
    }

    pub fn mark_final_blocks(
        &mut self,
        add_block_id: &BlockId,
        final_blocks: PreHashSet<BlockId>,
    ) -> Result<(), ConsensusError> {
        for block_id in final_blocks.into_iter() {
            // remove from gi_head
            if let Some(other_incomps) = self.gi_head.remove(&block_id) {
                for other_incomp in other_incomps.into_iter() {
                    if let Some(other_incomp_lst) = self.gi_head.get_mut(&other_incomp) {
                        other_incomp_lst.remove(&block_id);
                    }
                }
            }

            // mark as final and update latest_final_blocks_periods
            if let Some(BlockStatus::Active {
                a_block: final_block,
                ..
            }) = self.blocks_state.get_mut(&block_id)
            {
                massa_trace!("consensus.block_graph.add_block_to_graph.final", {
                    "hash": block_id
                });
                final_block.is_final = true;
                // remove from cliques
                let final_block_fitness = final_block.fitness;
                self.max_cliques.iter_mut().for_each(|c| {
                    if c.block_ids.remove(&block_id) {
                        c.fitness -= final_block_fitness;
                    }
                });
                self.max_cliques.retain(|c| !c.block_ids.is_empty()); // remove empty cliques
                if self.max_cliques.is_empty() {
                    // make sure at least one clique remains
                    self.max_cliques = vec![Clique {
                        block_ids: PreHashSet::<BlockId>::default(),
                        fitness: 0,
                        is_blockclique: true,
                    }];
                }
                // update latest final blocks
                if final_block.slot.period
                    > self.latest_final_blocks_periods[final_block.slot.thread as usize].1
                {
                    self.latest_final_blocks_periods[final_block.slot.thread as usize] =
                        (block_id, final_block.slot.period);
                }
                // update new final blocks list
                self.new_final_blocks.insert(block_id);
            } else {
                return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses updating final blocks adding {} - block {} is missing", add_block_id, block_id)));
            }
        }
        Ok(())
    }
}
