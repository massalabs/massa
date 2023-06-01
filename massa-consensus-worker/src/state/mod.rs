use std::{
    collections::{HashMap, VecDeque},
    vec,
};

use massa_consensus_exports::{
    block_graph_export::BlockGraphExport,
    block_status::{BlockStatus, ExportCompiledBlock, HeaderOrBlock},
    error::ConsensusError,
    ConsensusChannels, ConsensusConfig,
};
use massa_models::{
    active_block::ActiveBlock,
    address::Address,
    block::BlockGraphStatus,
    block_header::SecuredHeader,
    block_id::BlockId,
    clique::Clique,
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
    slot::Slot,
};
use massa_storage::Storage;
use massa_time::MassaTime;
use tracing::debug;

mod clique_computation;
mod graph;
mod process;
mod process_commands;
mod prune;
mod stats;
mod tick;
mod verifications;

#[derive(Clone)]
pub struct ConsensusState {
    /// Configuration
    pub config: ConsensusConfig,
    /// Channels to communicate with other modules
    pub channels: ConsensusChannels,
    /// Storage
    pub storage: Storage,
    /// Block ids of genesis blocks
    pub genesis_hashes: Vec<BlockId>,
    /// Incompatibility graph: maps a block id to the block ids it is incompatible with
    /// One entry per Active Block
    pub gi_head: PreHashMap<BlockId, PreHashSet<BlockId>>,
    /// All the cliques
    pub max_cliques: Vec<Clique>,
    /// ids of active blocks
    pub active_index: PreHashSet<BlockId>,
    /// ids of active blocks without ops
    pub active_index_without_ops: PreHashSet<BlockId>,
    /// Save of latest periods
    pub save_final_periods: Vec<u64>,
    /// One (block id, period) per thread
    pub latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// One `(block id, period)` per thread TODO not sure I understand the difference with `latest_final_blocks_periods`
    pub best_parents: Vec<(BlockId, u64)>,
    /// Every block we know about
    pub block_statuses: PreHashMap<BlockId, BlockStatus>,
    /// Ids of incoming blocks/headers
    pub incoming_index: PreHashSet<BlockId>,
    /// Used to limit the number of waiting and discarded blocks
    pub sequence_counter: u64,
    /// ids of waiting for slot blocks/headers
    pub waiting_for_slot_index: PreHashSet<BlockId>,
    /// ids of waiting for dependencies blocks/headers
    pub waiting_for_dependencies_index: PreHashSet<BlockId>,
    /// ids of discarded blocks
    pub discarded_index: PreHashSet<BlockId>,
    /// Blocks that need to be propagated
    pub to_propagate: PreHashMap<BlockId, Storage>,
    /// List of block ids we think are attack attempts
    pub attack_attempts: Vec<BlockId>,
    /// Newly final blocks
    pub new_final_blocks: PreHashSet<BlockId>,
    /// Newly stale block mapped to creator and slot
    pub new_stale_blocks: PreHashMap<BlockId, (Address, Slot)>,
    /// time at which the node was launched (used for desynchronization detection)
    pub launch_time: MassaTime,
    /// Final block stats `(time, creator, is_from_protocol)`
    pub final_block_stats: VecDeque<(MassaTime, Address, bool)>,
    /// Blocks that come from protocol used for stats and ids are removed when inserted in `final_block_stats`
    pub protocol_blocks: VecDeque<(MassaTime, BlockId)>,
    /// Stale block timestamp
    pub stale_block_stats: VecDeque<MassaTime>,
    /// the time span considered for stats
    pub stats_history_timespan: MassaTime,
    /// the time span considered for desynchronization detection
    pub stats_desync_detection_timespan: MassaTime,
    /// blocks we want
    pub wishlist: PreHashMap<BlockId, Option<SecuredHeader>>,
    /// previous blockclique notified to Execution
    pub prev_blockclique: PreHashMap<BlockId, Slot>,
    /// Blocks indexed by slot (used for multi-stake limiting). Blocks
    /// should be saved in this map when we receive the header or the full block directly.
    pub nonfinal_active_blocks_per_slot: HashMap<Slot, PreHashSet<BlockId>>,
}

impl ConsensusState {
    /// Get a full active block
    pub fn get_full_active_block(&self, block_id: &BlockId) -> Option<(&ActiveBlock, &Storage)> {
        match self.block_statuses.get(block_id) {
            Some(BlockStatus::Active { a_block, storage }) => Some((a_block.as_ref(), storage)),
            _ => None,
        }
    }

    /// Get a full active block
    ///
    /// Returns an error if it was not found
    fn try_get_full_active_block(
        &self,
        block_id: &BlockId,
    ) -> Result<(&ActiveBlock, &Storage), ConsensusError> {
        self.get_full_active_block(block_id).ok_or_else(|| {
            ConsensusError::ContainerInconsistency(format!("block {} is missing", block_id))
        })
    }

    pub fn get_clique_count(&self) -> usize {
        self.max_cliques.len()
    }

    /// get the blockclique (or final) block ID at a given slot, if any
    pub fn get_blockclique_block_at_slot(&self, slot: &Slot) -> Option<BlockId> {
        // List all blocks at this slot.
        // The list should be small: make a copy of it to avoid holding the storage lock.
        let blocks_at_slot = {
            let storage_read = self.storage.read_blocks();
            let returned = match storage_read.get_blocks_by_slot(slot) {
                Some(v) => v.clone(),
                None => return None,
            };
            returned
        };

        // search for the block in the blockclique
        let search_in_blockclique = blocks_at_slot
            .intersection(
                &self
                    .max_cliques
                    .iter()
                    .find(|c| c.is_blockclique)
                    .expect("expected one clique to be the blockclique")
                    .block_ids,
            )
            .next();
        if let Some(found_id) = search_in_blockclique {
            return Some(*found_id);
        }

        // block not found in the blockclique: search in the final blocks
        blocks_at_slot
            .into_iter()
            .find(|b_id| match self.block_statuses.get(b_id) {
                Some(BlockStatus::Active { a_block, .. }) => a_block.is_final,
                _ => false,
            })
    }

    /// get the latest blockclique (or final) block ID at a given slot, if any
    pub fn get_latest_blockclique_block_at_slot(&self, slot: &Slot) -> BlockId {
        let (mut best_block_id, mut best_block_period) = self
            .latest_final_blocks_periods
            .get(slot.thread as usize)
            .unwrap_or_else(|| panic!("unexpected not found latest final block period"));

        self.get_blockclique()
            .iter()
            .for_each(|id| match self.block_statuses.get(id) {
                Some(BlockStatus::Active {
                    a_block,
                    storage: _,
                }) => {
                    if a_block.is_final {
                        panic!(
                            "unexpected final block on getting latest blockclique block at slot"
                        );
                    }
                    if a_block.slot.thread == slot.thread
                        && a_block.slot.period < slot.period
                        && a_block.slot.period > best_block_period
                    {
                        best_block_period = a_block.slot.period;
                        best_block_id = *id;
                    }
                }
                _ => {
                    panic!("expected to find only active block but found another status")
                }
            });
        best_block_id
    }

    pub fn get_block_status(&self, block_id: &BlockId) -> BlockGraphStatus {
        match self.block_statuses.get(block_id) {
            None => BlockGraphStatus::NotFound,
            Some(BlockStatus::Active { a_block, .. }) => {
                if a_block.is_final {
                    BlockGraphStatus::Final
                } else if self
                    .max_cliques
                    .iter()
                    .find(|clique| clique.is_blockclique)
                    .expect("blockclique absent")
                    .block_ids
                    .contains(block_id)
                {
                    BlockGraphStatus::ActiveInBlockclique
                } else {
                    BlockGraphStatus::ActiveInAlternativeCliques
                }
            }
            Some(BlockStatus::Discarded { .. }) => BlockGraphStatus::Discarded,
            Some(BlockStatus::Incoming(_)) => BlockGraphStatus::Incoming,
            Some(BlockStatus::WaitingForDependencies { .. }) => {
                BlockGraphStatus::WaitingForDependencies
            }
            Some(BlockStatus::WaitingForSlot(_)) => BlockGraphStatus::WaitingForSlot,
        }
    }

    /// list the latest final blocks at the given slot
    ///
    /// exclusively used by `list_required_active_blocks`
    fn list_latest_final_blocks_at(
        &self,
        slot: Slot,
    ) -> Result<Vec<(BlockId, u64)>, ConsensusError> {
        let mut latest: Vec<Option<(BlockId, u64)>> = vec![None; self.config.thread_count as usize];
        for id in self.active_index.iter() {
            let (block, _storage) = self.try_get_full_active_block(id)?;
            if let Some((_, p)) = latest[block.slot.thread as usize] && block.slot.period < p {
                continue;
            }
            if block.is_final && block.slot <= slot {
                latest[block.slot.thread as usize] = Some((*id, block.slot.period));
            }
        }
        latest
            .into_iter()
            .enumerate()
            .map(|(thread, opt)| {
                opt.ok_or_else(|| {
                    ConsensusError::ContainerInconsistency(format!(
                        "could not find latest block for thread {}",
                        thread
                    ))
                })
            })
            .collect()
    }

    /// list the earliest blocks of the given block id list
    ///
    /// exclusively used by `list_required_active_blocks`
    fn list_earliest_blocks_of(
        &self,
        block_ids: &PreHashSet<BlockId>,
        end_slot: Option<Slot>,
    ) -> Result<Vec<(BlockId, u64)>, ConsensusError> {
        let mut earliest: Vec<Option<(BlockId, u64)>> =
            vec![None; self.config.thread_count as usize];
        for id in block_ids {
            let (block, _storage) = self.try_get_full_active_block(id)?;
            if let Some(slot) = end_slot && block.slot > slot {
                continue;
            }
            if let Some((_, p)) = earliest[block.slot.thread as usize] && block.slot.period > p {
                continue;
            }
            earliest[block.slot.thread as usize] = Some((*id, block.slot.period));
        }
        earliest
            .into_iter()
            .enumerate()
            .map(|(thread, opt)| {
                opt.ok_or_else(|| {
                    ConsensusError::ContainerInconsistency(format!(
                        "could not find earliest block for thread {}",
                        thread
                    ))
                })
            })
            .collect()
    }

    /// adds to the given container every active block coming after the lower bound
    ///
    /// exclusively used by `list_required_active_blocks`
    fn add_active_blocks_after(
        &self,
        kept_blocks: &mut PreHashSet<BlockId>,
        lower_bound: &[(BlockId, u64)],
        end_slot: Option<Slot>,
    ) {
        for id in self.active_index.iter() {
            if let Some((block, _storage)) = self.get_full_active_block(id) {
                if let Some(slot) = end_slot && block.slot > slot {
                    continue;
                }
                if block.slot.period >= lower_bound[block.slot.thread as usize].1 {
                    kept_blocks.insert(*id);
                }
            }
        }
    }

    /// list_required_active_blocks algo:
    ///
    /// if end_slot is None:
    ///      set effective_latest_finals to be the IDs of the self.latest_final_blocks
    /// else
    ///      set effective_latest_finals to be the IDs of the Active Final blocks that have the highest period in each thread but are before end_slot (included)
    ///
    /// create a kept_blocks list of block IDs to keep
    /// initialize it with effective_latest_finals as well as all the active blocks that are after the effective_latest_finals of their thread (included) (but before end_slot (included) if it is Some)
    ///
    /// do the following 2 times:
    ///      extend kept_blocks with the parents of the current kept_blocks
    ///      fill holes by adding to kept_blocks all the active block IDs whose slot is after the earliest kept_blocks of their thread (included) (but before end_slot (included) if it is Some)
    ///
    /// return kept_blocks
    pub fn list_required_active_blocks(
        &self,
        end_slot: Option<Slot>,
    ) -> Result<PreHashSet<BlockId>, ConsensusError> {
        // if an end_slot is provided compute the lastest final block for that given slot
        // if not use the latest_final_blocks_periods
        let effective_latest_finals: Vec<(BlockId, u64)> = if let Some(slot) = end_slot {
            self.list_latest_final_blocks_at(slot)?
        } else {
            self.latest_final_blocks_periods.clone()
        };

        // init kept_blocks using effective_latest_finals
        let mut kept_blocks: PreHashSet<BlockId> = effective_latest_finals
            .iter()
            .map(|(id, _period)| *id)
            .collect();

        // add all the active blocks that are after the effective_latest_finals of their thread
        self.add_active_blocks_after(&mut kept_blocks, &effective_latest_finals, end_slot);

        // do the following 2 times
        for _ in 0..2 {
            // extend kept_blocks with the parents of the current kept_blocks
            let mut cumulated_parents: PreHashSet<BlockId> = PreHashSet::default();
            for id in kept_blocks.iter() {
                let parents = self
                    .try_get_full_active_block(id)?
                    .0
                    .parents
                    .iter()
                    .map(|(id, _period)| *id);
                cumulated_parents.extend(parents);
            }
            kept_blocks.extend(cumulated_parents);
            // add all the active blocks whose slot is after the earliest kept_blocks of their thread
            let earliest_blocks = self.list_earliest_blocks_of(&kept_blocks, end_slot)?;
            self.add_active_blocks_after(&mut kept_blocks, &earliest_blocks, end_slot);
        }

        // check that we have the full blocks for every id we are about to return
        for id in kept_blocks.iter() {
            self.try_get_full_active_block(id)?;
        }

        // debug log for an easier diagnostic if needed
        debug!("list_required_active_blocks return: {:?}", kept_blocks);

        // return kept_blocks
        Ok(kept_blocks)
    }

    pub fn extract_block_graph_part(
        &self,
        slot_start: Option<Slot>,
        slot_end: Option<Slot>,
    ) -> Result<BlockGraphExport, ConsensusError> {
        let mut export = BlockGraphExport {
            genesis_blocks: self.genesis_hashes.clone(),
            active_blocks: PreHashMap::with_capacity(self.block_statuses.len()),
            discarded_blocks: PreHashMap::with_capacity(self.block_statuses.len()),
            best_parents: self.best_parents.clone(),
            latest_final_blocks_periods: self.latest_final_blocks_periods.clone(),
            gi_head: self.gi_head.clone(),
            max_cliques: self.max_cliques.clone(),
        };

        let filter = |&s| {
            if let Some(s_start) = slot_start {
                if s < s_start {
                    return false;
                }
            }
            if let Some(s_end) = slot_end {
                if s >= s_end {
                    return false;
                }
            }
            true
        };

        for (hash, block) in self.block_statuses.iter() {
            match block {
                BlockStatus::Discarded {
                    slot,
                    creator,
                    parents,
                    reason,
                    ..
                } => {
                    if filter(slot) {
                        export
                            .discarded_blocks
                            .insert(*hash, (reason.clone(), (*slot, *creator, parents.clone())));
                    }
                }
                BlockStatus::Active { a_block, storage } => {
                    if filter(&a_block.slot) {
                        let stored_block =
                            storage.read_blocks().get(hash).cloned().ok_or_else(|| {
                                ConsensusError::MissingBlock(format!(
                                    "missing block in BlockGraphExport::extract_from: {}",
                                    hash
                                ))
                            })?;
                        export.active_blocks.insert(
                            *hash,
                            ExportCompiledBlock {
                                header: stored_block.content.header,
                                children: a_block
                                    .children
                                    .iter()
                                    .map(|thread| {
                                        thread.keys().copied().collect::<PreHashSet<BlockId>>()
                                    })
                                    .collect(),
                                is_final: a_block.is_final,
                            },
                        );
                    }
                }
                _ => continue,
            }
        }

        Ok(export)
    }

    /// Gets all stored final blocks, not only the still-useful ones
    /// This is used when initializing Execution from Consensus.
    /// Since the Execution bootstrap snapshot is older than the Consensus snapshot,
    /// we might need to signal older final blocks for Execution to catch up.
    pub fn get_all_final_blocks(&self) -> HashMap<BlockId, (Slot, Storage)> {
        self.active_index
            .iter()
            .map(|b_id| {
                let block_infos = match self.block_statuses.get(b_id) {
                    Some(BlockStatus::Active { a_block, storage }) => {
                        (a_block.slot, storage.clone())
                    }
                    _ => panic!("active block missing"),
                };
                (*b_id, block_infos)
            })
            .collect()
    }

    /// get the current block wish list, including the operations hash.
    pub fn get_block_wishlist(
        &self,
    ) -> Result<PreHashMap<BlockId, Option<SecuredHeader>>, ConsensusError> {
        let mut wishlist = PreHashMap::<BlockId, Option<SecuredHeader>>::default();
        for block_id in self.waiting_for_dependencies_index.iter() {
            if let Some(BlockStatus::WaitingForDependencies {
                unsatisfied_dependencies,
                ..
            }) = self.block_statuses.get(block_id)
            {
                for unsatisfied_h in unsatisfied_dependencies.iter() {
                    match self.block_statuses.get(unsatisfied_h) {
                        Some(BlockStatus::WaitingForDependencies {
                            header_or_block: HeaderOrBlock::Header(header),
                            ..
                        }) => {
                            wishlist.insert(header.id, Some(header.clone()));
                        }
                        None => {
                            wishlist.insert(*unsatisfied_h, None);
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(wishlist)
    }

    /// Gets a block and all its descendants
    ///
    /// # Argument
    /// * hash : hash of the given block
    pub fn get_active_block_and_descendants(
        &self,
        block_id: &BlockId,
    ) -> Result<PreHashSet<BlockId>, ConsensusError> {
        let mut to_visit = vec![*block_id];
        let mut result = PreHashSet::<BlockId>::default();
        while let Some(visit_h) = to_visit.pop() {
            if !result.insert(visit_h) {
                continue; // already visited
            }
            match self.block_statuses.get(&visit_h) {
                Some(BlockStatus::Active { a_block, .. }) => {
                    a_block.as_ref()
                    .children.iter()
                    .for_each(|thread_children| to_visit.extend(thread_children.keys()))
                },
                _ => return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses iterating through descendants of {} - missing {}", block_id, visit_h))),
            }
        }
        Ok(result)
    }
}
