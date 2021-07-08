//! All information concerning blocks, the block graph and cliques is managed here.
use super::{config::ConsensusConfig, random_selector::RandomSelector};
use crate::error::ConsensusError;
use crypto::hash::Hash;
use crypto::signature::SignatureEngine;
use models::{Block, BlockHeader, BlockHeaderContent, SerializationContext, Slot};
use serde::{Deserialize, Serialize};
use std::mem;
use std::{
    collections::{hash_map, BTreeSet, HashMap, HashSet},
    u64,
};

#[derive(Debug, Clone)]
enum HeaderOrBlock {
    Header(BlockHeader),
    Block(Block),
}

impl HeaderOrBlock {
    pub fn get_slot(&self) -> Slot {
        match self {
            HeaderOrBlock::Header(header) => header.content.slot,
            HeaderOrBlock::Block(block) => block.header.content.slot,
        }
    }
}

#[derive(Debug, Clone)]
struct ActiveBlock {
    block: Block,
    parents: Vec<(Hash, u64)>, // one (hash, period) per thread ( if not genesis )
    children: Vec<HashMap<Hash, u64>>, // one HashMap<hash, period> per thread (blocks that need to be kept)
    dependencies: HashSet<Hash>,       // dependencies required for validity check
    is_final: bool,                    // true if final
    fitness: u64,                      // block fitness
    finality_depth: Option<u64>,       // largest in-clique descendant depth  (0 = not final, +delta_f0 = almost final)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscardReason {
    /// Block is invalid, either structurally, or because of some incompatibility.
    Invalid,
    /// Block is incompatible with a final block.
    Stale,
    /// Block has enough fitness.
    Final,
}

#[derive(Debug, Clone)]
enum BlockStatus {
    Incoming(HeaderOrBlock),
    WaitingForSlot(HeaderOrBlock),
    WaitingForDependencies {
        header_or_block: HeaderOrBlock,
        unsatisfied_dependencies: HashSet<Hash>, // includes self if it's only a header
    },
    Active(ActiveBlock),
    Discarded {
        header: BlockHeader,
        reason: DiscardReason,
        sequence_number: u64,
    },
}

pub struct MaxClique {
    block_hashes: HashSet<Hash>,
    fitness: u64,
}

pub struct BlockGraph {
    cfg: ConsensusConfig,
    serialization_context: SerializationContext,
    genesis_hashes: Vec<Hash>,
    sequence_counter: u64,
    block_statuses: HashMap<Hash, BlockStatus>,
    latest_final_blocks_periods: Vec<(Hash, u64)>,
    best_parents: Vec<Hash>,
    gi_head: HashMap<Hash, HashSet<Hash>>,
    max_cliques: Vec<MaxClique>,
    blockclique_index: usize,
    to_propagate: HashMap<Hash, BlockHeader>,
}

enum CheckOutcome {
    Proceed {
        parents_hash_period: Vec<(Hash, u64)>,
        dependencies: HashSet<Hash>,
        incompatibilities: HashSet<Hash>,
        inherited_incompatibilities_count: usize,
    },
    Discard(DiscardReason),
    WaitForSlot,
    WaitForDependencies(HashSet<Hash>),
}

/// Creates genesis block in given thread.
///
/// # Arguments
/// * cfg: consensus configuration
/// * serialization_context: ref to a SerializationContext instance
/// * thread_number: thread in wich we want a genesis block
fn create_genesis_block(
    cfg: &ConsensusConfig,
    serialization_context: &SerializationContext,
    thread_number: u8,
) -> Result<(Hash, Block), ConsensusError> {
    let mut signature_engine = SignatureEngine::new();
    let private_key = cfg.genesis_key;
    let public_key = signature_engine.derive_public_key(&private_key);
    let (header_hash, header) = BlockHeader::new_signed(
        &mut signature_engine,
        &private_key,
        BlockHeaderContent {
            creator: public_key,
            slot: Slot::new(0, thread_number),
            parents: Vec::new(),
            out_ledger_hash: Hash::hash("Hello world !".as_bytes()),
            operation_merkle_root: Hash::hash("Hello world !".as_bytes()),
        },
        &serialization_context,
    )?;
    Ok((
        header_hash,
        Block {
            header,
            operations: Vec::new(),
        },
    ))
}

impl BlockGraph {
    /// Creates a new block_graph.
    ///
    /// # Argument
    /// * cfg : consensus configuration.
    /// * serialization_context: SerializationContext instance
    pub fn new(
        cfg: ConsensusConfig,
        serialization_context: SerializationContext,
    ) -> Result<Self, ConsensusError> {
        // load genesis blocks
        let mut block_statuses = HashMap::new();
        let mut block_hashes = Vec::with_capacity(cfg.thread_count as usize);
        for thread in 0u8..cfg.thread_count {
            let (hash, block) = create_genesis_block(&cfg, &serialization_context, thread)
                .map_err(|_err| ConsensusError::GenesisCreationError)?;
            block_hashes.push(hash);
            block_statuses.insert(
                hash,
                BlockStatus::Active(ActiveBlock {
                    block,
                    parents: Vec::new(),
                    children: vec![HashMap::new(); cfg.thread_count as usize],
                    dependencies: HashSet::new(),
                    is_final: true,
                    fitness: 1u64, // TODO compute block fitness
                    finality_depth: None,
                }),
            );
        }

        Ok(BlockGraph {
            cfg,
            serialization_context,
            sequence_counter: 0,
            genesis_hashes: block_hashes.clone(),
            block_statuses,
            latest_final_blocks_periods: block_hashes.iter().map(|h| (*h, 0 as u64)).collect(),
            best_parents: block_hashes,
            gi_head: HashMap::new(),
            max_cliques: vec![MaxClique {
                block_hashes: HashSet::new(),
                fitness: 0,
            }],
            blockclique_index: 0,
            to_propagate: Default::default(),
        })
    }

    /// Returns hash and resulting discarded blocks
    ///
    /// # Arguments
    /// * val : dummy value used to generate dummy hash
    /// * slot : generated block is in slot slot.
    pub fn create_block(&self, val: String, slot: Slot) -> Result<(Hash, Block), ConsensusError> {
        let mut signature_engine = SignatureEngine::new();
        let (public_key, private_key) = self
            .cfg
            .nodes
            .get(self.cfg.current_node_index as usize)
            .and_then(|(public_key, private_key)| Some((public_key.clone(), private_key.clone())))
            .ok_or(ConsensusError::KeyError)?;

        let example_hash = Hash::hash(&val.as_bytes());

        let (hash, header) = BlockHeader::new_signed(
            &mut signature_engine,
            &private_key,
            BlockHeaderContent {
                creator: public_key,
                slot: slot,
                parents: self.best_parents.clone(),
                out_ledger_hash: example_hash,
                operation_merkle_root: example_hash,
            },
            &self.serialization_context,
        )?;

        Ok((
            hash,
            Block {
                header,
                operations: Vec::new(),
            },
        ))
    }

    /// Gets lastest final blocks (hash, period) for each thread.
    pub fn get_latest_final_blocks_periods(&self) -> &Vec<(Hash, u64)> {
        &self.latest_final_blocks_periods
    }

    /// Gets whole block corresponding to given hash, if it is active.
    ///
    /// # Argument
    /// * hash : header's hash of the given block.
    pub fn get_active_block(&self, hash: Hash) -> Option<&Block> {
        match BlockGraph::get_full_active_block(&self.block_statuses, hash) {
            Some(ActiveBlock { block, .. }) => Some(&block),
            _ => None,
        }
    }

    // signal new slot
    pub fn slot_tick(
        &mut self,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<(), ConsensusError> {
        // list all elements for which the time has come
        let to_process: BTreeSet<(Slot, Hash)> = self
            .block_statuses
            .iter()
            .filter_map(|(hash, block_status)| match block_status {
                BlockStatus::WaitingForSlot(header_or_block) => {
                    let slot = header_or_block.get_slot();
                    if Some(slot) <= current_slot {
                        Some((slot, *hash))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        // process those elements
        self.rec_process(to_process, selector, current_slot)?;

        Ok(())
    }

    pub fn incoming_header(
        &mut self,
        hash: Hash,
        header: BlockHeader,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<(), ConsensusError> {
        // ignore genesis blocks
        if self.genesis_hashes.contains(&hash) {
            return Ok(());
        }

        let mut to_ack: BTreeSet<(Slot, Hash)> = BTreeSet::new();
        match self.block_statuses.entry(hash) {
            // if absent => add as Incoming, call rec_ack on it
            hash_map::Entry::Vacant(vac) => {
                to_ack.insert((header.content.slot, hash));
                vac.insert(BlockStatus::Incoming(HeaderOrBlock::Header(header)));
            }
            hash_map::Entry::Occupied(mut occ) => match occ.get_mut() {
                BlockStatus::Discarded {
                    sequence_number, ..
                } => {
                    // promote if discarded
                    *sequence_number = BlockGraph::new_sequence_number(&mut self.sequence_counter);
                }
                _ => {}
            },
        }

        // process
        self.rec_process(to_ack, selector, current_slot)?;

        Ok(())
    }

    pub fn incoming_block(
        &mut self,
        hash: Hash,
        block: Block,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<(), ConsensusError> {
        // ignore genesis blocks
        if self.genesis_hashes.contains(&hash) {
            return Ok(());
        }

        let mut to_ack: BTreeSet<(Slot, Hash)> = BTreeSet::new();
        match self.block_statuses.entry(hash) {
            // if absent => add as Incoming, call rec_ack on it
            hash_map::Entry::Vacant(vac) => {
                to_ack.insert((block.header.content.slot, hash));
                vac.insert(BlockStatus::Incoming(HeaderOrBlock::Block(block)));
            }
            hash_map::Entry::Occupied(mut occ) => match occ.get_mut() {
                BlockStatus::Discarded {
                    sequence_number, ..
                } => {
                    // promote if discarded
                    *sequence_number = BlockGraph::new_sequence_number(&mut self.sequence_counter);
                }
                BlockStatus::WaitingForSlot(header_or_block) => {
                    // promote to full block
                    *header_or_block = HeaderOrBlock::Block(block);
                }
                BlockStatus::WaitingForDependencies {
                    header_or_block,
                    unsatisfied_dependencies,
                } => {
                    // promote to full block and satisfy self-dependency
                    if unsatisfied_dependencies.remove(&hash) {
                        // a dependency was satisfied: process
                        to_ack.insert((block.header.content.slot, hash));
                    }
                    *header_or_block = HeaderOrBlock::Block(block);
                }
                _ => return Ok(()),
            },
        }

        // process
        self.rec_process(to_ack, selector, current_slot)?;

        Ok(())
    }

    fn new_sequence_number(sequence_counter: &mut u64) -> u64 {
        let res = *sequence_counter;
        *sequence_counter += 1;
        res
    }

    // acknowledge a set of items recursively
    fn rec_process(
        &mut self,
        mut to_ack: BTreeSet<(Slot, Hash)>,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<(), ConsensusError> {
        // order processing by (slot, hash)
        while let Some((_slot, hash)) = to_ack.pop_first() {
            to_ack.extend(self.process(hash, selector, current_slot)?)
        }
        Ok(())
    }

    // ack a single item, return a set of items to re-ack
    fn process(
        &mut self,
        hash: Hash,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<BTreeSet<(Slot, Hash)>, ConsensusError> {
        // list items to reprocess
        let mut reprocess = BTreeSet::new();

        // control all the waiting states and try to get a valid block
        let (
            valid_block,
            valid_block_parents_hash_period,
            valid_block_deps,
            valid_block_incomp,
            valid_block_inherited_incomp_count,
        ) = match self.block_statuses.get(&hash) {
            None => return Ok(BTreeSet::new()), // disappeared before being processed: do nothing

            // discarded: do nothing
            Some(BlockStatus::Discarded { .. }) => return Ok(BTreeSet::new()),

            // already active: do nothing
            Some(BlockStatus::Active(_)) => return Ok(BTreeSet::new()),

            // incoming header
            Some(BlockStatus::Incoming(HeaderOrBlock::Header(_))) => {
                // remove header
                let header = if let Some(BlockStatus::Incoming(HeaderOrBlock::Header(header))) =
                    self.block_statuses.remove(&hash)
                {
                    header
                } else {
                    return Err(ConsensusError::ContainerInconsistency);
                };
                match self.check_header(hash, &header, selector, current_slot)? {
                    CheckOutcome::Proceed { .. } => {
                        // set as waiting dependencies
                        let mut dependencies = HashSet::new();
                        dependencies.insert(hash); // add self as unsatisfied
                        self.block_statuses.insert(
                            hash,
                            BlockStatus::WaitingForDependencies {
                                header_or_block: HeaderOrBlock::Header(header),
                                unsatisfied_dependencies: dependencies,
                            },
                        );
                        return Ok(BTreeSet::new());
                    }
                    CheckOutcome::WaitForDependencies(mut dependencies) => {
                        // set as waiting dependencies
                        dependencies.insert(hash); // add self as unsatisfied
                        self.block_statuses.insert(
                            hash,
                            BlockStatus::WaitingForDependencies {
                                header_or_block: HeaderOrBlock::Header(header),
                                unsatisfied_dependencies: dependencies,
                            },
                        );
                        return Ok(BTreeSet::new());
                    }
                    CheckOutcome::WaitForSlot => {
                        // make it wait for slot
                        self.block_statuses.insert(
                            hash,
                            BlockStatus::WaitingForSlot(HeaderOrBlock::Header(header)),
                        );
                        return Ok(BTreeSet::new());
                    }
                    CheckOutcome::Discard(reason) => {
                        // discard
                        self.block_statuses.insert(
                            hash,
                            BlockStatus::Discarded {
                                header: header,
                                reason,
                                sequence_number: BlockGraph::new_sequence_number(
                                    &mut self.sequence_counter,
                                ),
                            },
                        );
                        return Ok(BTreeSet::new());
                    }
                }
            }

            // incoming block
            Some(BlockStatus::Incoming(HeaderOrBlock::Block(_))) => {
                let block = if let Some(BlockStatus::Incoming(HeaderOrBlock::Block(block))) =
                    self.block_statuses.remove(&hash)
                {
                    block
                } else {
                    return Err(ConsensusError::ContainerInconsistency);
                };
                match self.check_block(hash, &block, selector, current_slot)? {
                    CheckOutcome::Proceed {
                        parents_hash_period,
                        dependencies,
                        incompatibilities,
                        inherited_incompatibilities_count,
                    } => {
                        // block is valid: remove it from Incoming and return it
                        (
                            block,
                            parents_hash_period,
                            dependencies,
                            incompatibilities,
                            inherited_incompatibilities_count,
                        )
                    }
                    CheckOutcome::WaitForDependencies(dependencies) => {
                        // set as waiting dependencies
                        self.block_statuses.insert(
                            hash,
                            BlockStatus::WaitingForDependencies {
                                header_or_block: HeaderOrBlock::Block(block),
                                unsatisfied_dependencies: dependencies,
                            },
                        );
                        return Ok(BTreeSet::new());
                    }
                    CheckOutcome::WaitForSlot => {
                        // set as waiting for slot
                        self.block_statuses.insert(
                            hash,
                            BlockStatus::WaitingForSlot(HeaderOrBlock::Block(block)),
                        );
                        return Ok(BTreeSet::new());
                    }
                    CheckOutcome::Discard(reason) => {
                        // add to discard
                        self.block_statuses.insert(
                            hash,
                            BlockStatus::Discarded {
                                header: block.header,
                                reason,
                                sequence_number: BlockGraph::new_sequence_number(
                                    &mut self.sequence_counter,
                                ),
                            },
                        );
                        return Ok(BTreeSet::new());
                    }
                }
            }

            Some(BlockStatus::WaitingForSlot(header_or_block)) => {
                let slot = header_or_block.get_slot();
                if Some(slot) > current_slot {
                    // in the future: ignore
                    return Ok(BTreeSet::new());
                }
                // send back as incoming and ask for reprocess
                if let Some(BlockStatus::WaitingForSlot(header_or_block)) =
                    self.block_statuses.remove(&hash)
                {
                    self.block_statuses
                        .insert(hash, BlockStatus::Incoming(header_or_block));
                    reprocess.insert((slot, hash));
                    return Ok(reprocess);
                } else {
                    return Err(ConsensusError::ContainerInconsistency);
                };
            }

            Some(BlockStatus::WaitingForDependencies {
                unsatisfied_dependencies,
                ..
            }) => {
                if !unsatisfied_dependencies.is_empty() {
                    // still has unsatisfied depdendencies: ignore
                    return Ok(BTreeSet::new());
                }
                // send back as incoming and ask for reprocess
                if let Some(BlockStatus::WaitingForDependencies {
                    header_or_block, ..
                }) = self.block_statuses.remove(&hash)
                {
                    reprocess.insert((header_or_block.get_slot(), hash));
                    self.block_statuses
                        .insert(hash, BlockStatus::Incoming(header_or_block));
                    return Ok(reprocess);
                } else {
                    return Err(ConsensusError::ContainerInconsistency);
                }
            }
        };

        // add block to graph
        self.add_block_to_graph(
            hash,
            valid_block_parents_hash_period,
            valid_block,
            valid_block_deps,
            valid_block_incomp,
            valid_block_inherited_incomp_count,
        )?;

        // if the block was added, update linked dependencies and mark satisfied ones for recheck
        if let Some(BlockStatus::Active(active)) = self.block_statuses.get(&hash) {
            self.to_propagate
                .insert(hash.clone(), active.block.header.clone());
            for (itm_hash, itm_status) in self.block_statuses.iter_mut() {
                if let BlockStatus::WaitingForDependencies {
                    header_or_block,
                    unsatisfied_dependencies,
                } = itm_status
                {
                    if unsatisfied_dependencies.remove(&hash) {
                        // a dependency was satisfied: retry
                        reprocess.insert((header_or_block.get_slot(), *itm_hash));
                    }
                }
            }
        }

        Ok(reprocess)
    }

    /// Gets whole ActiveBlock corresponding to given hash
    ///
    /// # Argument
    /// * hash : header's hash of the given block.
    fn get_full_active_block(
        block_statuses: &HashMap<Hash, BlockStatus>,
        hash: Hash,
    ) -> Option<&ActiveBlock> {
        match block_statuses.get(&hash) {
            Some(BlockStatus::Active(active_block)) => Some(&active_block),
            _ => None,
        }
    }

    /// Gets a block and all its desencants
    ///
    /// # Argument
    /// * hash : hash of the given block
    fn get_active_block_and_descendants(
        &self,
        hash: Hash,
    ) -> Result<HashSet<Hash>, ConsensusError> {
        let mut to_visit = vec![hash];
        let mut result: HashSet<Hash> = HashSet::new();
        while let Some(visit_h) = to_visit.pop() {
            if !result.insert(visit_h) {
                continue; // already visited
            }
            BlockGraph::get_full_active_block(&self.block_statuses, visit_h)
                .ok_or(ConsensusError::ContainerInconsistency)?
                .children
                .iter()
                .for_each(|thread_children| to_visit.extend(thread_children.keys()));
        }
        Ok(result)
    }

    fn check_header(
        &self,
        hash: Hash,
        header: &BlockHeader,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<CheckOutcome, ConsensusError> {
        let mut parents: Vec<(Hash, u64)> = Vec::with_capacity(self.cfg.thread_count as usize);
        let mut deps = HashSet::new();
        let mut incomp = HashSet::new();
        let mut missing_deps = HashSet::new();

        // basic structural checks
        if header.content.parents.len() != (self.cfg.thread_count as usize)
            || header.content.slot.period == 0
            || header.content.slot.thread >= self.cfg.thread_count
        {
            return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
        }

        // check that is older than the latest final block in that thread
        // Note: this excludes genesis blocks
        if header.content.slot.period
            <= self.latest_final_blocks_periods[header.content.slot.thread as usize].1
        {
            return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
        }

        // check if block slot is too much in the future
        if let Some(cur_slot) = current_slot {
            if header.content.slot.period
                > cur_slot
                    .period
                    .saturating_add(self.cfg.future_block_processing_max_periods)
            {
                return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
            }
        }

        // check if it was the creator's turn to create this block
        // note: do this AFTER TooMuchInTheFuture checks
        //       to avoid doing too many draws to check blocks in the distant future
        if header.content.creator != self.cfg.nodes[selector.draw(header.content.slot) as usize].0 {
            // it was not the creator's turn to create a block for this slot
            return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
        }

        // check if block is in the future: queue it
        // note: do it after testing signature + draw to prevent queue flooding/DoS
        // note: Some(x) > None
        if Some(header.content.slot) > current_slot {
            return Ok(CheckOutcome::WaitForSlot);
        }

        // TODO check if we already have a block for that slot
        // TODO denounce ? see issue #101

        // list parents and ensure they are present
        let parent_set: HashSet<Hash> = header.content.parents.iter().copied().collect();
        deps.extend(&parent_set);
        for parent_thread in 0u8..self.cfg.thread_count {
            let parent_hash = header.content.parents[parent_thread as usize];
            match self.block_statuses.get(&parent_hash) {
                Some(BlockStatus::Discarded { .. }) => {
                    // parent is discarded
                    return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
                }
                Some(BlockStatus::Active(parent)) => {
                    // parent is active

                    // check that the parent is from an earlier slot in the right thread
                    if parent.block.header.content.slot.thread != parent_thread
                        || parent.block.header.content.slot >= header.content.slot
                    {
                        return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
                    }

                    // inherit parent incompatibilities
                    // and ensure parents are mutually compatible
                    if let Some(p_incomp) = self.gi_head.get(&parent_hash) {
                        if !p_incomp.is_disjoint(&parent_set) {
                            return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
                        }
                        incomp.extend(p_incomp);
                    }

                    parents.push((parent_hash, parent.block.header.content.slot.period));
                }
                _ => {
                    // parent is missing or queued
                    if self.genesis_hashes.contains(&parent_hash) {
                        // forbid depending on discarded genesis block
                        return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
                    }
                    missing_deps.insert(parent_hash);
                }
            }
        }
        if !missing_deps.is_empty() {
            return Ok(CheckOutcome::WaitForDependencies(missing_deps));
        }
        let inherited_incomp_count = incomp.len();

        // check the topological consistency of the parents
        {
            let mut gp_max_slots = vec![0u64; self.cfg.thread_count as usize];
            for parent_i in 0..self.cfg.thread_count {
                let (parent_h, parent_period) = parents[parent_i as usize];
                let parent = self
                    .get_active_block(parent_h)
                    .ok_or(ConsensusError::ContainerInconsistency)?;
                if parent_period < gp_max_slots[parent_i as usize] {
                    // a parent is earlier than a block known by another parent in that thread
                    return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
                }
                gp_max_slots[parent_i as usize] = parent_period;
                if parent_period == 0 {
                    // genesis
                    continue;
                }
                for gp_i in 0..self.cfg.thread_count {
                    if gp_i == parent_i {
                        continue;
                    }
                    let gp_h = parent.header.content.parents[gp_i as usize];
                    deps.insert(gp_h);
                    match self.block_statuses.get(&gp_h) {
                        // this grandpa is discarded
                        Some(BlockStatus::Discarded { .. }) => {
                            return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
                        }
                        // this grandpa is active
                        Some(BlockStatus::Active(gp)) => {
                            if gp.block.header.content.slot.period > gp_max_slots[gp_i as usize] {
                                if gp_i < parent_i {
                                    return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
                                }
                                gp_max_slots[gp_i as usize] = gp.block.header.content.slot.period;
                            }
                        }
                        // this grandpa is missing or queued
                        _ => {
                            if self.genesis_hashes.contains(&gp_h) {
                                // forbid depending on discarded genesis block
                                return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
                            }
                            missing_deps.insert(gp_h);
                        }
                    }
                }
            }
        }
        if !missing_deps.is_empty() {
            return Ok(CheckOutcome::WaitForDependencies(missing_deps));
        }

        // get parent in own thread
        let parent_in_own_thread = BlockGraph::get_full_active_block(
            &self.block_statuses,
            parents[header.content.slot.thread as usize].0,
        )
        .ok_or(ConsensusError::ContainerInconsistency)?;

        // thread incompatibility test
        parent_in_own_thread.children[header.content.slot.thread as usize]
            .keys()
            .filter(|&sibling_h| *sibling_h != hash)
            .try_for_each(|&sibling_h| {
                incomp.extend(self.get_active_block_and_descendants(sibling_h)?);
                Result::<(), ConsensusError>::Ok(())
            })?;

        // grandpa incompatibility test
        for tau in (0u8..self.cfg.thread_count).filter(|&t| t != header.content.slot.thread) {
            // for each parent in a different thread tau
            // traverse parent's descendance in tau
            let mut to_explore = vec![(0usize, header.content.parents[tau as usize])];
            while let Some((cur_gen, cur_h)) = to_explore.pop() {
                let cur_b = BlockGraph::get_full_active_block(&self.block_statuses, cur_h)
                    .ok_or(ConsensusError::ContainerInconsistency)?;

                // traverse but do not check up to generation 1
                if cur_gen <= 1 {
                    to_explore.extend(
                        cur_b.children[tau as usize]
                            .keys()
                            .map(|&c_h| (cur_gen + 1, c_h)),
                    );
                    continue;
                }

                // check if the parent in tauB has a strictly lower period number than B's parent in tauB
                // note: cur_b cannot be genesis at gen > 1
                if BlockGraph::get_full_active_block(
                    &self.block_statuses,
                    cur_b.block.header.content.parents[header.content.slot.thread as usize],
                )
                .ok_or(ConsensusError::ContainerInconsistency)?
                .block
                .header
                .content
                .slot
                .period
                    < parent_in_own_thread.block.header.content.slot.period
                {
                    // GPI detected
                    incomp.extend(self.get_active_block_and_descendants(cur_h)?);
                } // otherwise, cur_b and its descendants cannot be GPI with the block: don't traverse
            }
        }

        // check if the block is incompatible with a parent
        if !incomp.is_disjoint(&parents.iter().map(|(h, _p)| *h).collect()) {
            return Ok(CheckOutcome::Discard(DiscardReason::Invalid));
        }

        // check if the block is incompatible with a final block
        if !incomp.is_disjoint(
            &self
                .block_statuses
                .iter()
                .filter_map(|(h, s)| {
                    if let BlockStatus::Active(a) = s {
                        if a.is_final {
                            return Some(*h);
                        }
                    }
                    None
                })
                .collect(),
        ) {
            return Ok(CheckOutcome::Discard(DiscardReason::Stale));
        }

        Ok(CheckOutcome::Proceed {
            parents_hash_period: parents,
            dependencies: deps,
            incompatibilities: incomp,
            inherited_incompatibilities_count: inherited_incomp_count,
        })
    }

    fn check_block(
        &self,
        hash: Hash,
        block: &Block,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<CheckOutcome, ConsensusError> {
        let mut deps;
        let mut incomp;
        let parents;
        let inherited_incomp_count;

        // check header
        match self.check_header(hash, &block.header, selector, current_slot)? {
            CheckOutcome::Proceed {
                parents_hash_period,
                dependencies,
                incompatibilities,
                inherited_incompatibilities_count,
            } => {
                parents = parents_hash_period;
                deps = dependencies;
                incomp = incompatibilities;
                inherited_incomp_count = inherited_incompatibilities_count;
            }
            outcome => return Ok(outcome),
        }

        //TODO check block

        // TODO operation incompatibility test (see issue #102)

        Ok(CheckOutcome::Proceed {
            parents_hash_period: parents,
            dependencies: deps,
            incompatibilities: incomp,
            inherited_incompatibilities_count: inherited_incomp_count,
        })
    }

    /// Computes max cliques of compatible blocks
    fn compute_max_cliques(&self) -> Vec<HashSet<Hash>> {
        let mut max_cliques: Vec<HashSet<Hash>> = Vec::new();

        // algorithm adapted from IK_GPX as summarized in:
        //   Cazals et al., "A note on the problem of reporting maximal cliques"
        //   Theoretical Computer Science, 2008
        //   https://doi.org/10.1016/j.tcs.2008.05.010

        // stack: r, p, x
        let mut stack: Vec<(HashSet<Hash>, HashSet<Hash>, HashSet<Hash>)> = vec![(
            HashSet::new(),
            self.gi_head.keys().cloned().collect(),
            HashSet::new(),
        )];
        while let Some((r, mut p, mut x)) = stack.pop() {
            if p.is_empty() && x.is_empty() {
                max_cliques.push(r);
                continue;
            }
            // choose the pivot vertex following the GPX scheme:
            // u_p = node from (p \/ x) that maximizes the cardinality of (P \ Neighbors(u_p, GI))
            let &u_p = p
                .union(&x)
                .max_by_key(|&u| {
                    p.difference(&(&self.gi_head[u] | &vec![*u].into_iter().collect()))
                        .count()
                })
                .unwrap(); // p was checked to be non-empty before

            // iterate over u_set = (p /\ Neighbors(u_p, GI))
            let u_set: HashSet<Hash> =
                &p & &(&self.gi_head[&u_p] | &vec![u_p].into_iter().collect());
            for u_i in u_set.into_iter() {
                p.remove(&u_i);
                let u_i_set: HashSet<Hash> = vec![u_i].into_iter().collect();
                let comp_n_u_i: HashSet<Hash> = &self.gi_head[&u_i] | &u_i_set;
                stack.push((&r | &u_i_set, &p - &comp_n_u_i, &x - &comp_n_u_i));
                x.insert(u_i);
            }
        }
        if max_cliques.is_empty() {
            // make sure at least one clique remains
            max_cliques = vec![HashSet::new()];
        }
        max_cliques
    }

    fn add_block_to_graph(
        &mut self,
        hash: Hash,
        parents_hash_period: Vec<(Hash, u64)>,
        block: Block,
        deps: HashSet<Hash>,
        incomp: HashSet<Hash>,
        inherited_incomp_count: usize,
    ) -> Result<(), ConsensusError> {
        // add block to status structure
        self.block_statuses.insert(
            hash,
            BlockStatus::Active(ActiveBlock {
                parents: parents_hash_period.clone(),
                dependencies: deps,
                block: block.clone(),
                children: vec![HashMap::new(); self.cfg.thread_count as usize],
                is_final: false,
                fitness: 1u64, // TODO
                finality_depth: None,
            }),
        );
        for (parent_h, _parent_period) in parents_hash_period.iter() {
            if let Some(BlockStatus::Active(a_parent)) = self.block_statuses.get_mut(parent_h) {
                a_parent.children[block.header.content.slot.thread as usize]
                    .insert(hash, block.header.content.slot.period);
            } else {
                return Err(ConsensusError::ContainerInconsistency);
            }
        }

        // add incompatibilities to gi_head
        for incomp_h in incomp.iter() {
            self.gi_head
                .get_mut(incomp_h)
                .ok_or(ConsensusError::MissingBlock)?
                .insert(hash);
        }
        self.gi_head.insert(hash, incomp.clone());

        // max cliques update
        if incomp.len() == inherited_incomp_count {
            // clique optimization routine:
            //   the block only has incompatibilities inherited from its parents
            //   therfore it is not forking and can simply be added to the cliques it is compatible with
            self.max_cliques
                .iter_mut()
                .filter(|MaxClique { block_hashes, .. }| incomp.is_disjoint(block_hashes))
                .for_each(|MaxClique { block_hashes, .. }| drop(block_hashes.insert(hash)));
        } else {
            // fully recompute max cliques
            self.max_cliques = self
                .compute_max_cliques()
                .into_iter()
                .map(|c| MaxClique {
                    block_hashes: c,
                    fitness: 0,
                })
                .collect();
        }

        // compute clique fitnesses and find blockclique
        let mut blockclique_hashsum = num::BigUint::default();
        self.blockclique_index = 0usize;
        for (
            clique_i,
            MaxClique {
                block_hashes,
                fitness,
                ..
            },
        ) in self.max_cliques.iter_mut().enumerate()
        {
            *fitness = 0;
            let mut hash_sum = num::BigUint::default();
            for block_h in block_hashes.iter() {
                *fitness = fitness
                    .checked_add(
                        BlockGraph::get_full_active_block(&self.block_statuses, *block_h)
                            .ok_or(ConsensusError::ContainerInconsistency)?
                            .fitness,
                    )
                    .ok_or(ConsensusError::FitnessOverflow)?;
                hash_sum += num::BigUint::from_bytes_be(&block_h.to_bytes());
            }
            if clique_i == self.blockclique_index
                || (*fitness, std::cmp::Reverse(hash_sum))
                    > (
                        self.max_cliques[self.blockclique_index].fitness,
                        std::cmp::Reverse(blockclique_hashsum),
                    )
            {
                self.blockclique_index = clique_i;
                blockclique_hashsum = hash_sum;
            }
        }

        // update best parents
        {
            let blockclique = &self.max_cliques[self.blockclique_index].block_hashes;
            let mut parents_updated = 0u8;
            for block_h in blockclique.iter() {
                let block_a = BlockGraph::get_full_active_block(&self.block_statuses, *block_h)
                    .ok_or(ConsensusError::ContainerInconsistency)?;
                if blockclique.is_disjoint(
                    &block_a.children[block_a.block.header.content.slot.thread as usize]
                        .keys()
                        .copied()
                        .collect(),
                ) {
                    self.best_parents[block_a.block.header.content.slot.thread as usize] = *block_h;
                    parents_updated += 1;
                    if parents_updated == self.cfg.thread_count {
                        break;
                    }
                }
            }
        }

        // list stale blocks
        let stale_blocks = {
            let fitnesss_threshold = self.max_cliques[self.blockclique_index]
                .fitness
                .saturating_sub(self.cfg.delta_f0);
            // iterate from largest to smallest to minimize reallocations
            let mut indices: Vec<usize> = (0..self.max_cliques.len()).collect();
            indices.sort_unstable_by_key(|&i| {
                std::cmp::Reverse(self.max_cliques[i].block_hashes.len())
            });
            let mut high_set: HashSet<Hash> = HashSet::new();
            let mut low_set: HashSet<Hash> = HashSet::new();
            let mut keep_mask = vec![true; self.max_cliques.len()];
            for clique_i in indices.into_iter() {
                if self.max_cliques[clique_i].fitness >= fitnesss_threshold {
                    high_set.extend(self.max_cliques[clique_i].block_hashes);
                } else {
                    low_set.extend(self.max_cliques[clique_i].block_hashes);
                    keep_mask[clique_i] = false;
                }
            }
            let mut clique_i = 0;
            self.max_cliques.retain(|_| {
                clique_i += 1;
                keep_mask[clique_i - 1]
            });
            clique_i = 0;
            &low_set - &high_set
        };
        // mark stale blocks
        for stale_block_hash in stale_blocks.into_iter() {
            if let Some(BlockStatus::Active(active_block)) =
                self.block_statuses.remove(&stale_block_hash)
            {
                if active_block.is_final {
                    return Err(ConsensusError::ContainerInconsistency);
                }

                // remove from gi_head
                if let Some(other_incomps) = self.gi_head.remove(&stale_block_hash) {
                    for other_incomp in other_incomps.into_iter() {
                        if let Some(other_incomp_lst) = self.gi_head.get_mut(&other_incomp) {
                            other_incomp_lst.remove(&stale_block_hash);
                        }
                    }
                }

                // remove from cliques
                self.max_cliques
                    .iter_mut()
                    .for_each(|c| drop(c.block_hashes.remove(&stale_block_hash)));
                self.max_cliques.retain(|c| !c.block_hashes.is_empty()); // remove empty cliques
                if self.max_cliques.is_empty() {
                    // make sure at least one clique remains
                    self.max_cliques = vec![MaxClique {
                        block_hashes: HashSet::new(),
                        fitness: 0,
                    }];
                }

                // remove from parent's children
                for (parent_h, _parent_period) in active_block.parents.iter() {
                    if let Some(BlockStatus::Active(ActiveBlock { children, .. })) =
                        self.block_statuses.get_mut(parent_h)
                    {
                        children[block.header.content.slot.thread as usize].remove(&hash);
                    }
                }

                // mark as stale
                self.block_statuses.insert(
                    stale_block_hash,
                    BlockStatus::Discarded {
                        header: active_block.block.header,
                        reason: DiscardReason::Stale,
                        sequence_number: BlockGraph::new_sequence_number(
                            &mut self.sequence_counter,
                        ),
                    },
                );
            } else {
                return Err(ConsensusError::ContainerInconsistency);
            }
        }

        // list final blocks
        let final_blocks = {
            // reset finality_depth
            for block_status in self.block_statuses.values_mut() {
                if let BlockStatus::Active(ActiveBlock { finality_depth, .. }) = block_status {
                    *finality_depth = None;
                }
            }

            //  short-circuiting intersection of cliques from smallest to largest
            let mut indices: Vec<usize> = (0..self.max_cliques.len()).collect();
            indices.sort_unstable_by_key(|&i| self.max_cliques[i].block_hashes.len());
            let mut final_candidates = self.max_cliques[indices[0]].block_hashes.clone();
            if indices.len() > 1 {
                for i in 1..indices.len() {
                    final_candidates.retain(|h| self.max_cliques[i].block_hashes.contains(h));
                    if final_candidates.is_empty() {
                        break;
                    }
                }
            }

            // optimization: restrict search to cliques with high enough fitness
            // note: makes finality_depth unavailable when cliques are too small
            //indices.retain(|&i| self.max_cliques[i].fitness > self.cfg.delta_f0);

            // check in-clique total descendants fitness
            indices.sort_unstable_by_key(|&i| self.max_cliques[i].fitness);
            let mut final_blocks: HashSet<Hash> = HashSet::new();
            for clique_i in indices.into_iter().rev() {
                // check in cliques from highest to lowest fitness
                if final_candidates.is_empty() {
                    // no more final candidates
                    break;
                }
                let clique = &self.max_cliques[clique_i].block_hashes;
                // sum the descendence fitness for each candidate
                let cloned_candidates = final_candidates.clone();
                for candidate_h in cloned_candidates.into_iter() {
                    let mut visited: HashSet<Hash> = HashSet::new();
                    let mut stack: Vec<Hash> = vec![candidate_h];
                    let mut is_final = false;
                    let mut desc_rem_fitness = self.cfg.delta_f0; // remaining required fitness
                    while let Some(h) = stack.pop() {
                        let root = h == candidate_h;
                        if !root {
                            if !visited.insert(h) {
                                continue;
                            }
                        }
                        let b = BlockGraph::get_full_active_block(&self.block_statuses, h)
                            .ok_or(ConsensusError::MissingBlock)?;
                        b.children.iter().for_each(|c_set| {
                            stack.extend(clique.intersection(&c_set.keys().copied().collect()))
                        });
                        if !root {
                            if let Some(v) = desc_rem_fitness.checked_sub(b.fitness) {
                                desc_rem_fitness = v;
                            } else {
                                // block is final
                                final_candidates.remove(&candidate_h);
                                final_blocks.insert(candidate_h);
                                break;
                            }
                        }
                    }
                    if let Some(BlockStatus::Active(ActiveBlock { finality_depth, .. })) =
                        self.block_statuses.get_mut(&candidate_h)
                    {
                        if is_final {
                            *finality_depth = None;
                        } else if let Some(f) = finality_depth {
                            *f = std::cmp::min(*f, self.cfg.delta_f0 - desc_rem_fitness);
                        } else {
                            *finality_depth = Some(self.cfg.delta_f0 - desc_rem_fitness);
                        }
                    }
                }
            }
            final_blocks
        };
        // mark final blocks and update latest_final_blocks_periods
        for final_block_hash in final_blocks.into_iter() {
            // remove from gi_head
            if let Some(other_incomps) = self.gi_head.remove(&final_block_hash) {
                for other_incomp in other_incomps.into_iter() {
                    if let Some(other_incomp_lst) = self.gi_head.get_mut(&other_incomp) {
                        other_incomp_lst.remove(&final_block_hash);
                    }
                }
            }

            // remove from cliques
            self.max_cliques
                .iter_mut()
                .for_each(|c| drop(c.block_hashes.remove(&final_block_hash)));
            self.max_cliques.retain(|c| !c.block_hashes.is_empty()); // remove empty cliques
            if self.max_cliques.is_empty() {
                // make sure at least one clique remains
                self.max_cliques = vec![MaxClique {
                    block_hashes: HashSet::new(),
                    fitness: 0,
                }];
            }

            // mark as final and update latest_final_blocks_periods
            if let Some(BlockStatus::Active(ActiveBlock {
                block: final_block,
                is_final,
                ..
            })) = self.block_statuses.get_mut(&final_block_hash)
            {
                *is_final = true;
                if final_block.header.content.slot.period
                    > self.latest_final_blocks_periods
                        [final_block.header.content.slot.thread as usize]
                        .1
                {
                    self.latest_final_blocks_periods
                        [final_block.header.content.slot.thread as usize] = (
                        final_block_hash.clone(),
                        final_block.header.content.slot.period,
                    );
                }
            } else {
                return Err(ConsensusError::ContainerInconsistency);
            }
        }

        Ok(())
    }

    // prune active blocks and return final blocks, return discarded final blocks
    fn prune_active(
        &mut self,
        current_slot: Option<Slot>,
    ) -> Result<HashMap<Hash, Block>, ConsensusError> {
        // list all active blocks
        let active_blocks: HashSet<Hash> = self
            .block_statuses
            .iter()
            .filter_map(|(h, bs)| match bs {
                BlockStatus::Active(_) => Some(*h),
                _ => None,
            })
            .collect();

        let mut retain_active: HashSet<Hash> = HashSet::new();

        // retain all non-final active blocks and their dependencies
        for (hash, block_status) in self.block_statuses.iter() {
            if let BlockStatus::Active(ActiveBlock {
                is_final,
                dependencies,
                ..
            }) = block_status
            {
                if !*is_final {
                    retain_active.extend(dependencies);
                    retain_active.insert(*hash);
                }
            }
        }

        // retain last final blocks
        retain_active.extend(
            self.latest_final_blocks_periods
                .iter()
                .map(|(h, _)| h.clone()),
        );

        // retain best parents
        retain_active.extend(&self.best_parents);

        // grow with parents & fill thread holes twice
        for _ in 0..1 {
            // retain the parents of the selected blocks
            let retain_clone = retain_active.clone();

            for retain_h in retain_clone.into_iter() {
                retain_active.extend(
                    self.get_active_block(retain_h)
                        .ok_or(ConsensusError::ContainerInconsistency)?
                        .header
                        .content
                        .parents
                        .iter()
                        .copied(),
                )
            }

            // find earliest kept slots in each thread
            let mut earliest_retained_periods: Vec<u64> = self
                .latest_final_blocks_periods
                .iter()
                .map(|(_, p)| *p)
                .collect();
            for retain_h in retain_active.iter() {
                let retain_slot = &self
                    .get_active_block(*retain_h)
                    .ok_or(ConsensusError::ContainerInconsistency)?
                    .header
                    .content
                    .slot;
                earliest_retained_periods[retain_slot.thread as usize] = std::cmp::min(
                    earliest_retained_periods[retain_slot.thread as usize],
                    retain_slot.period,
                );
            }

            // fill up from the latest final block back to the earliest for each thread
            for thread in 0..self.cfg.thread_count {
                let mut cursor = self.latest_final_blocks_periods[thread as usize].0;
                while let Some(c_block) = self.get_active_block(cursor) {
                    if c_block.header.content.slot.period
                        < earliest_retained_periods[thread as usize]
                    {
                        break;
                    }
                    retain_active.insert(cursor);
                    if c_block.header.content.parents.len() < self.cfg.thread_count as usize {
                        // genesis
                        break;
                    }
                    cursor = c_block.header.content.parents[thread as usize];
                }
            }
        }

        // TODO keep enough blocks in each thread to test for still-valid, non-reusable transactions
        // see issue #98

        // remove unused final active blocks
        let mut discarded_finals: HashMap<Hash, Block> = HashMap::new();
        for discard_active_h in active_blocks.difference(&retain_active) {
            let discarded_active = if let Some(BlockStatus::Active(discarded_active)) =
                self.block_statuses.remove(discard_active_h)
            {
                discarded_active
            } else {
                return Err(ConsensusError::ContainerInconsistency);
            };

            // remove from parent's children
            for (parent_h, _parent_period) in discarded_active.parents.iter() {
                if let Some(BlockStatus::Active(ActiveBlock { children, .. })) =
                    self.block_statuses.get_mut(parent_h)
                {
                    children[discarded_active.block.header.content.slot.thread as usize]
                        .remove(&discard_active_h);
                }
            }

            // mark as final
            self.block_statuses.insert(
                *discard_active_h,
                BlockStatus::Discarded {
                    header: discarded_active.block.header.clone(),
                    reason: DiscardReason::Final,
                    sequence_number: BlockGraph::new_sequence_number(&mut self.sequence_counter),
                },
            );

            discarded_finals.insert(*discard_active_h, discarded_active.block);
        }

        Ok(discarded_finals)
    }

    fn prune_waiting_for_dependencies(&mut self) -> Result<(), ConsensusError> {
        let mut to_discard: HashMap<Hash, Option<DiscardReason>> = HashMap::new();
        let mut to_keep: HashSet<Hash> = HashSet::new();

        // list items that are older than the latest final blocks in their threads or have deps that are discarded
        {
            for (hash, block_status) in self.block_statuses.iter() {
                if let BlockStatus::WaitingForDependencies {
                    header_or_block,
                    unsatisfied_dependencies,
                    ..
                } = block_status
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
                                DiscardReason::Invalid => {
                                    discard_reason = Some(DiscardReason::Invalid);
                                    break;
                                }
                                DiscardReason::Stale => discard_reason = Some(DiscardReason::Stale),
                                DiscardReason::Final => discard_reason = Some(DiscardReason::Stale),
                            }
                        }
                    }
                    if discarded_dep_found {
                        to_discard.insert(*hash, discard_reason);
                        continue;
                    }

                    // is at least as old as the latest final block in its thread => discard as stale
                    let slot = header_or_block.get_slot();
                    if slot.period <= self.latest_final_blocks_periods[slot.thread as usize].1 {
                        to_discard.insert(*hash, Some(DiscardReason::Stale));
                        continue;
                    }

                    // otherwise, mark as to_keep
                    to_keep.insert(*hash);
                }
            }
        }

        // discard in chain and because of limited size
        while !to_keep.is_empty() {
            // mark entries as to_discard and remove them from to_keep
            for hash in to_keep.clone().into_iter() {
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
                                Some(DiscardReason::Invalid) => {
                                    discard_reason = Some(DiscardReason::Invalid);
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
            if to_keep.len() > self.cfg.max_dependency_blocks {
                let remove_elt = to_keep
                    .iter()
                    .filter_map(|hash| {
                        if let Some(BlockStatus::WaitingForDependencies {
                            header_or_block, ..
                        }) = self.block_statuses.get(hash)
                        {
                            return Some((header_or_block.get_slot(), *hash));
                        }
                        None
                    })
                    .reduce(std::cmp::min);
                if let Some((_slot, hash)) = remove_elt {
                    to_keep.remove(&hash);
                    to_discard.insert(hash, None);
                    continue;
                }
            }

            // nothing happened: stop loop
            break;
        }

        // transition states to Discarded if there is a reason, otherwise just drop
        for (hash, reason_opt) in to_discard.drain() {
            if let Some(BlockStatus::WaitingForDependencies {
                header_or_block, ..
            }) = self.block_statuses.remove(&hash)
            {
                let header = match header_or_block {
                    HeaderOrBlock::Header(h) => h,
                    HeaderOrBlock::Block(b) => b.header,
                };
                // transition to Discarded only if there is a reason
                if let Some(reason) = reason_opt {
                    self.block_statuses.insert(
                        hash,
                        BlockStatus::Discarded {
                            header,
                            reason,
                            sequence_number: BlockGraph::new_sequence_number(
                                &mut self.sequence_counter,
                            ),
                        },
                    );
                }
            }
        }

        Ok(())
    }

    fn prune_discarded(&mut self) -> Result<(), ConsensusError> {
        let mut discard_hashes: Vec<(u64, Hash)> = self
            .block_statuses
            .iter()
            .filter_map(|(hash, block_status)| {
                if let BlockStatus::Discarded {
                    sequence_number, ..
                } = block_status
                {
                    return Some((*sequence_number, *hash));
                }
                None
            })
            .collect();
        if discard_hashes.len() <= self.cfg.max_discarded_blocks {
            return Ok(());
        }
        discard_hashes.sort_unstable();
        discard_hashes.truncate(self.cfg.max_discarded_blocks);
        discard_hashes
            .into_iter()
            .take(self.cfg.max_discarded_blocks)
            .for_each(|(_period, hash)| {
                self.block_statuses.remove(&hash);
            });
        Ok(())
    }

    // prune and return final blocks, return discarded final blocks
    pub fn prune(
        &mut self,
        current_slot: Option<Slot>,
    ) -> Result<HashMap<Hash, Block>, ConsensusError> {
        // Step 1: discard final blocks that are not useful to the graph anymore and return them
        let discarded_finals = self.prune_active(current_slot)?;
        

        // Step 2: prune dependency waiting blocks
        self.prune_waiting_for_dependencies()?;

        // Step 3: prune discarded
        self.prune_discarded()?;

        Ok(discarded_finals)
    }

    // get the current block wishlist
    pub fn get_block_wishlist(&self) -> Result<Vec<Hash>, ConsensusError> {
        let mut wishlist = vec![];
        for block_status in self.block_statuses.values() {
            if let BlockStatus::WaitingForDependencies {
                unsatisfied_dependencies,
                ..
            } = block_status
            {
                for unsatisfied_h in unsatisfied_dependencies.iter() {
                    if let Some(BlockStatus::WaitingForDependencies {
                        header_or_block: HeaderOrBlock::Block(_),
                        ..
                    }) = self.block_statuses.get(unsatisfied_h)
                    {
                        // the full block is already available
                        continue;
                    }
                    wishlist.push(*unsatisfied_h);
                }
            }
        }
        Ok(wishlist)
    }

    // Get the headers to be propagated.
    // Must be called by the consensus worker within `block_db_changed`.
    pub fn get_headers_to_propagate(&mut self) -> HashMap<Hash, BlockHeader> {
        mem::replace(&mut self.to_propagate, Default::default())
    }
}

/*


#[cfg(test)]
mod tests {
    use crypto::signature::SignatureEngine;
    use time::UTime;

    use super::BlockGraph;
    use super::*;
    use crate::{config::ConsensusConfig, random_selector::RandomSelector};

    fn example_consensus_config() -> (ConsensusConfig, SerializationContext) {
        let secp = SignatureEngine::new();
        let genesis_key = SignatureEngine::generate_random_private_key();
        let mut nodes = Vec::new();
        for _ in 0..2 {
            let private_key = SignatureEngine::generate_random_private_key();
            let public_key = secp.derive_public_key(&private_key);
            nodes.push((public_key, private_key));
        }
        let thread_count: u8 = 2;
        let max_block_size = 1024 * 1024;
        let max_operations_per_block = 1024;
        (
            ConsensusConfig {
                genesis_timestamp: UTime::now().unwrap(),
                thread_count,
                t0: 32.into(),
                selection_rng_seed: 42,
                genesis_key,
                nodes,
                current_node_index: 0,
                max_discarded_blocks: 10,
                future_block_processing_max_periods: 3,
                max_future_processing_blocks: 10,
                max_dependency_blocks: 10,
                delta_f0: 5,
                disable_block_creation: true,
                max_block_size,
                max_operations_per_block,
            },
            SerializationContext {
                max_block_size,
                max_block_operations: max_operations_per_block,
                parent_count: thread_count,
                max_peer_list_length: 128,
                max_message_size: 3 * 1024 * 1024,
            },
        )
    }

    fn create_standalone_block(
        cfg: &ConsensusConfig,
        graph: &mut BlockGraph,
        val: String,
        slot: Slot,
        parents: Vec<Hash>,
        selector: &mut RandomSelector,
    ) -> (Hash, Block) {
        let mut signature_engine = SignatureEngine::new();
        let example_hash = Hash::hash(&val.as_bytes());
        let mut parents = parents.clone();
        if parents.len() == 0 {
            parents = graph.best_parents.clone();
        }
        let (_, serialization_context) = example_consensus_config();

        let (public_key, private_key) = cfg.nodes[selector.draw(slot) as usize];

        let (hash, header) = BlockHeader::new_signed(
            &mut signature_engine,
            &private_key,
            BlockHeaderContent {
                creator: public_key,
                slot,
                parents,
                out_ledger_hash: example_hash,
                operation_merkle_root: example_hash,
            },
            &serialization_context,
        )
        .unwrap();

        (
            hash,
            Block {
                header,
                operations: Vec::new(),
            },
        )
    }

    #[test]
    fn test_parent_in_the_future() {
        let (cfg, serialization_context) = example_consensus_config();
        let mut block_graph = BlockGraph::new(cfg.clone(), serialization_context.clone()).unwrap();
        let mut selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");

        let (hash_2, block_2) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(2, 0),
            Vec::new(),
            &mut selector,
        );

        block_graph
            .acknowledge_block(hash_2, block_2, &mut selector, Slot::new(1000, 0))
            .unwrap();

        let (hash_1, block_1) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(1, 0),
            Vec::new(),
            &mut selector,
        );

        match block_graph.acknowledge_block(hash_1, block_1, &mut selector, Slot::new(1000, 0)) {
            Ok(_) => panic!("Corrupted block has been acknowledged"),
            Err(BlockAcknowledgeError::InvalidParents(_)) => {}
            Err(e) => panic!(format!("unexpected error {:?}", e)),
        }
    }

    #[test]
    fn test_parents_in_incompatible_cliques() {
        let (cfg, serialization_context) = example_consensus_config();
        let mut block_graph = BlockGraph::new(cfg.clone(), serialization_context.clone()).unwrap();
        let mut selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");

        let genesis = block_graph.best_parents.clone();

        let (hash_1, block_1) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(1, 0),
            genesis.clone(),
            &mut selector,
        );
        block_graph
            .acknowledge_block(hash_1, block_1, &mut selector, Slot::new(1000, 0))
            .unwrap();

        let (hash_2, block_2) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(2, 0),
            genesis.clone(),
            &mut selector,
        );
        block_graph
            .acknowledge_block(hash_2, block_2, &mut selector, Slot::new(1000, 0))
            .unwrap();

        // from that point we have two incompatible clique

        // block_3 is in clique 1
        let parents = vec![hash_1, genesis[1]];
        let (hash_3, block_3) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(1, 1),
            parents,
            &mut selector,
        );
        block_graph
            .acknowledge_block(hash_3, block_3, &mut selector, Slot::new(1000, 0))
            .unwrap();

        // parent in thread 0 is in clique 2 and parent in thread 1 is in clique 1
        let incompatible_parents = vec![hash_2, hash_3];
        let (hash_4, block_4) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(2, 1),
            incompatible_parents,
            &mut selector,
        );

        match block_graph.acknowledge_block(hash_4, block_4, &mut selector, Slot::new(1000, 0)) {
            Ok(_) => panic!("Corrupted block has been acknowledged"),
            Err(BlockAcknowledgeError::InvalidParents(_)) => {}
            Err(e) => panic!(format!("unexpected error {:?}", e)),
        }
    }

    /// Generate 2n blocks in 2 treads using best_parents
    /// all added to block_graph
    fn generate_blocks(
        cfg: &ConsensusConfig,
        mut block_graph: &mut BlockGraph,
        mut selector: &mut RandomSelector,
        n: u64,
        start_slots: (u64, u64),
    ) {
        for i in 0..n {
            // in thread 0
            let parents = block_graph.best_parents.clone();
            let (hash, block) = create_standalone_block(
                &cfg,
                &mut block_graph,
                "42".into(),
                Slot::new(start_slots.0 + i, 0),
                parents,
                &mut selector,
            );
            block_graph
                .acknowledge_block(
                    hash,
                    block,
                    &mut selector,
                    Slot::new(start_slots.0 + i + 5, 0),
                )
                .unwrap();

            // in thread 1

            let parents = block_graph.best_parents.clone();
            let (hash, block) = create_standalone_block(
                &cfg,
                &mut block_graph,
                "42".into(),
                Slot::new(start_slots.1 + i, 1),
                parents,
                &mut selector,
            );

            block_graph
                .acknowledge_block(
                    hash,
                    block,
                    &mut selector,
                    Slot::new(start_slots.0 + i + 5, 0),
                )
                .unwrap();
        }
    }

    fn extend_thread(
        cfg: &ConsensusConfig,
        block_graph: &mut BlockGraph,
        selector: &mut RandomSelector,
        n: u64,
        parents: Vec<Hash>,
        slot: Slot,
    ) {
        let mut current_parents = parents.clone();
        let mut current_period = slot.period;
        for _ in 0..n {
            let (hash, block) = create_standalone_block(
                &cfg,
                block_graph,
                "42".into(),
                Slot::new(current_period, slot.thread),
                current_parents.clone(),
                selector,
            );
            block_graph
                .acknowledge_block(
                    hash,
                    block,
                    selector,
                    Slot::new(current_period, slot.thread),
                )
                .unwrap();
            current_period += 1;
            current_parents[slot.thread as usize] = hash;
        }
    }

    #[test]
    fn test_thread_incompatibility() {
        let (mut cfg, serialization_context) = example_consensus_config();
        // ensure eliminated blocks remain in discard list
        cfg.max_discarded_blocks = 40;

        let mut block_graph = BlockGraph::new(cfg.clone(), serialization_context.clone()).unwrap();
        let mut selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");

        // generating two incompatible cliques
        let genesis = block_graph.best_parents.clone();

        let (hash_1, block_1) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(1, 0),
            genesis.clone(),
            &mut selector,
        );
        block_graph
            .acknowledge_block(hash_1, block_1, &mut selector, Slot::new(1000, 0))
            .unwrap();

        let (hash_2, block_2) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(1, 1),
            genesis.clone(),
            &mut selector,
        );
        block_graph
            .acknowledge_block(hash_2, block_2, &mut selector, Slot::new(1000, 0))
            .unwrap();

        let (hash_3, block_3) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(2, 0),
            genesis.clone(),
            &mut selector,
        );
        block_graph
            .acknowledge_block(hash_3, block_3, &mut selector, Slot::new(1000, 0))
            .unwrap();

        let parents = block_graph.best_parents.clone();
        if hash_1 > hash_3 {
            assert_eq!(parents[0], hash_3)
        } else {
            assert_eq!(parents[0], hash_1)
        }
        assert_eq!(parents[1], hash_2);

        assert!(if let Some(h) = block_graph.gi_head.get(&hash_3) {
            h.contains(&hash_1)
        } else {
            panic!("missign hash in gi_head")
        });

        assert_eq!(block_graph.max_cliques.len(), 2);

        for clique in block_graph.max_cliques.clone() {
            if clique.contains(&hash_1) && clique.contains(&hash_3) {
                panic!("incompatible bloocks in the same clique")
            }
        }

        extend_thread(
            &cfg,
            &mut block_graph,
            &mut selector,
            3,
            vec![hash_1, hash_2],
            Slot::new(3, 0),
        );
        assert!(if let Some(h) = block_graph.gi_head.get(&hash_3) {
            h.contains(&block_graph.best_parents[0])
        } else {
            panic!("missing block in clique")
        });

        let parents = vec![block_graph.best_parents[0].clone(), hash_2];
        extend_thread(
            &cfg,
            &mut block_graph,
            &mut selector,
            30,
            parents,
            Slot::new(8, 0),
        );

        assert_eq!(block_graph.max_cliques.len(), 1);

        // clique should have been deleted by now
        let parents = vec![hash_3, hash_2];
        let (hash_4, block_4) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(40, 0),
            parents,
            &mut selector,
        );

        match block_graph.acknowledge_block(hash_4, block_4, &mut selector, Slot::new(1000, 0)) {
            Ok(_) => panic!("Corrupted block has been acknowledged"),
            Err(BlockAcknowledgeError::InvalidParents(s)) => {
                println!("{}", s);
            }
            Err(e) => panic!(format!("unexpected error {:?}", e)),
        }
    }

    #[test]
    fn test_parents() {
        let (cfg, serialization_context) = example_consensus_config();
        let mut block_graph = BlockGraph::new(cfg.clone(), serialization_context.clone()).unwrap();
        let mut selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");

        let genesis = block_graph.best_parents.clone();

        // generate two normal blocks in each thread
        generate_blocks(&cfg, &mut block_graph, &mut selector, 2, (1, 1));

        let parents = block_graph.best_parents.clone();

        let (hash_1, block_1) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(3, 0),
            vec![parents[0], genesis[1]],
            &mut selector,
        );

        match block_graph.acknowledge_block(hash_1, block_1, &mut selector, Slot::new(1000, 0)) {
            Ok(_) => panic!("Corrupted block has been acknowledged"),
            Err(BlockAcknowledgeError::InvalidParents(s)) => {
                println!("{}", s);
            }
            Err(e) => panic!(format!("unexpected error {:?}", e)),
        }

        // block 2
        let (hash_2, block_2) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(3, 1),
            vec![genesis[0], genesis[0]],
            &mut selector,
        );

        match block_graph.acknowledge_block(hash_2, block_2, &mut selector, Slot::new(1000, 0)) {
            Ok(_) => panic!("Corrupted block has been acknowledged"),
            Err(BlockAcknowledgeError::InvalidParents(s)) => {
                println!("{}", s);
            }
            Err(e) => panic!(format!("unexpected error {:?}", e)),
        }
    }

    #[test]
    fn test_grandpa_incompatibility() {
        let (cfg, serialization_context) = example_consensus_config();
        let mut block_graph = BlockGraph::new(cfg.clone(), serialization_context.clone()).unwrap();
        let mut selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");
        let genesis = block_graph.best_parents.clone();

        let (hash_1, block_1) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(1, 0),
            vec![genesis[0], genesis[1]],
            &mut selector,
        );
        block_graph
            .acknowledge_block(hash_1, block_1, &mut selector, Slot::new(1000, 0))
            .unwrap();

        let (hash_2, block_2) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(1, 1),
            vec![genesis[0], genesis[1]],
            &mut selector,
        );
        block_graph
            .acknowledge_block(hash_2, block_2, &mut selector, Slot::new(1000, 0))
            .unwrap();

        let (hash_3, block_3) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(2, 0),
            vec![hash_1, genesis[1]],
            &mut selector,
        );
        block_graph
            .acknowledge_block(hash_3, block_3, &mut selector, Slot::new(1000, 0))
            .unwrap();

        let (hash_4, block_4) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(2, 1),
            vec![genesis[0], hash_2],
            &mut selector,
        );
        block_graph
            .acknowledge_block(hash_4, block_4, &mut selector, Slot::new(1000, 0))
            .unwrap();

        assert!(if let Some(h) = block_graph.gi_head.get(&hash_4) {
            h.contains(&hash_3)
        } else {
            panic!("missign block in gi_head")
        });

        assert_eq!(block_graph.max_cliques.len(), 2);

        for clique in block_graph.max_cliques.clone() {
            if clique.contains(&hash_3) && clique.contains(&hash_4) {
                panic!("incompatible blocks in the same clique")
            }
        }

        let parents = block_graph.best_parents.clone();
        if hash_4 > hash_3 {
            assert_eq!(parents[0], hash_3)
        } else {
            assert_eq!(parents[1], hash_4)
        }

        let mut latest_extra_blocks: VecDeque<Hash> = VecDeque::new();
        for extend_i in 0..33 {
            let parents = block_graph.best_parents.clone();
            let (hash_ext, block_ext) = create_standalone_block(
                &cfg,
                &mut block_graph,
                "42".into(),
                Slot::new(3 + extend_i, 0),
                parents,
                &mut selector,
            );
            block_graph
                .acknowledge_block(hash_ext, block_ext, &mut selector, Slot::new(1000, 0))
                .unwrap();
            latest_extra_blocks.push_back(hash_ext);
            while latest_extra_blocks.len() > cfg.delta_f0 as usize + 1 {
                latest_extra_blocks.pop_front();
            }
        }
        let latest_extra_blocks: HashSet<Hash> = latest_extra_blocks.into_iter().collect();
        assert_eq!(
            block_graph.max_cliques,
            vec![latest_extra_blocks],
            "wrong cliques"
        );
    }

    #[test]
    fn test_clique_calculation() {
        let (cfg, serialization_context) = example_consensus_config();
        let mut block_graph = BlockGraph::new(cfg, serialization_context).unwrap();
        let hashes: Vec<Hash> = vec![
            "VzCRpnoZVYY1yQZTXtVQbbxwzdu6hYtdCUZB5BXWSabsiXyfP",
            "JnWwNHRR1tUD7UJfnEFgDB4S4gfDTX2ezLadr7pcwuZnxTvn1",
            "xtvLedxC7CigAPytS5qh9nbTuYyLbQKCfbX8finiHsKMWH6SG",
            "2Qs9sSbc5sGpVv5GnTeDkTKdDpKhp4AgCVT4XFcMaf55msdvJN",
            "2VNc8pR4tNnZpEPudJr97iNHxXbHiubNDmuaSzrxaBVwKXxV6w",
            "2bsrYpfLdvVWAJkwXoJz1kn4LWshdJ6QjwTrA7suKg8AY3ecH1",
            "kfUeGj3ZgBprqFRiAQpE47dW5tcKTAueVaWXZquJW6SaPBd4G",
        ]
        .into_iter()
        .map(|h| Hash::from_bs58_check(h).unwrap())
        .collect();
        block_graph.gi_head = vec![
            (0, vec![1, 2, 3, 4]),
            (1, vec![0]),
            (2, vec![0]),
            (3, vec![0]),
            (4, vec![0]),
            (5, vec![6]),
            (6, vec![5]),
        ]
        .into_iter()
        .map(|(idx, lst)| (hashes[idx], lst.into_iter().map(|i| hashes[i]).collect()))
        .collect();
        let computed_sets = block_graph.compute_max_cliques();

        let expected_sets: Vec<HashSet<Hash>> = vec![
            vec![1, 2, 3, 4, 5],
            vec![1, 2, 3, 4, 6],
            vec![0, 5],
            vec![0, 6],
        ]
        .into_iter()
        .map(|lst| lst.into_iter().map(|i| hashes[i]).collect())
        .collect();

        assert_eq!(computed_sets.len(), expected_sets.len());
        for expected in expected_sets.into_iter() {
            assert!(computed_sets.iter().any(|v| v == &expected));
        }
    }

    #[test]
    fn test_old_stale() {
        let (cfg, serialization_context) = example_consensus_config();
        let mut block_graph = BlockGraph::new(cfg.clone(), serialization_context.clone()).unwrap();
        let mut selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");

        let genesis = block_graph.best_parents.clone();

        // generate two normal blocks in each thread
        generate_blocks(&cfg, &mut block_graph, &mut selector, 40, (1, 1));

        let (hash_1, block_1) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(1, 0),
            genesis,
            &mut selector,
        );

        match block_graph.acknowledge_block(hash_1, block_1, &mut selector, Slot::new(1000, 0)) {
            Ok(_) => panic!("Corrupted block has been acknowledged"),
            Err(BlockAcknowledgeError::TooOld) => {}
            Err(e) => panic!(format!("unexpected error {:?}", e)),
        }
    }

    #[test]
    fn test_queueing() {
        let (cfg, serialization_context) = example_consensus_config();
        let mut block_graph = BlockGraph::new(cfg.clone(), serialization_context.clone()).unwrap();
        let mut selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");

        let genesis = block_graph.best_parents.clone();

        // generate two normal blocks in each thread
        generate_blocks(&cfg, &mut block_graph, &mut selector, 2, (1, 1));

        // create a block that will be a missing dependency
        let (hash_miss, _block_miss) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(3, 0),
            genesis.clone(),
            &mut selector,
        );

        // create a block that depends on the missing dep
        let (hash_dep, block_dep) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(4, 0),
            vec![hash_miss, genesis[1]],
            &mut selector,
        );

        // make sure the dependency problem is detected
        match block_graph.acknowledge_block(hash_dep, block_dep, &mut selector, Slot::new(1000, 0))
        {
            Ok(_) => panic!("block with missing dependency acknowledged"),
            Err(BlockAcknowledgeError::MissingDependencies(_block, deps)) => {
                if deps != vec![hash_miss].into_iter().collect() {
                    panic!("wrong missing dependencies");
                }
            }
            Err(e) => panic!(format!("unexpected error {:?}", e)),
        };
    }

    #[test]
    fn test_doubles() {
        let (cfg, serialization_context) = example_consensus_config();
        let mut block_graph = BlockGraph::new(cfg.clone(), serialization_context.clone()).unwrap();
        let mut selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");

        // generate two normal blocks in each thread
        generate_blocks(&cfg, &mut block_graph, &mut selector, 40, (1, 1));

        let parents = block_graph.best_parents.clone();
        let (hash_1, block_1) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(42, 0),
            parents,
            &mut selector,
        );

        block_graph
            .acknowledge_block(hash_1, block_1.clone(), &mut selector, Slot::new(1000, 0))
            .unwrap();

        // second time processing same block
        match block_graph.acknowledge_block(
            hash_1,
            block_1.clone(),
            &mut selector,
            Slot::new(1000, 0),
        ) {
            Ok(_) => panic!("Corrupted block has been acknowledged"),
            Err(BlockAcknowledgeError::AlreadyAcknowledged) => {}
            Err(e) => panic!(format!("unexpected error {:?}", e)),
        };

        // third time processing same block
        match block_graph.acknowledge_block(
            hash_1,
            block_1.clone(),
            &mut selector,
            Slot::new(1000, 0),
        ) {
            Ok(_) => panic!("Corrupted block has been acknowledged"),
            Err(BlockAcknowledgeError::AlreadyAcknowledged) => {}
            Err(e) => panic!(format!("unexpected error {:?}", e)),
        };
    }

    #[test]
    fn test_double_staking() {
        let (cfg, serialization_context) = example_consensus_config();
        let mut block_graph = BlockGraph::new(cfg.clone(), serialization_context.clone()).unwrap();
        let mut selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");

        // generate two normal blocks in each thread
        generate_blocks(&cfg, &mut block_graph, &mut selector, 40, (1, 1));

        let parents = block_graph.best_parents.clone();
        let (hash_1, block_1) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "42".into(),
            Slot::new(42, 0),
            parents.clone(),
            &mut selector,
        );

        block_graph
            .acknowledge_block(hash_1, block_1.clone(), &mut selector, Slot::new(1000, 0))
            .unwrap();

        // same creator, same slot, different block
        let (hash_2, block_2) = create_standalone_block(
            &cfg,
            &mut block_graph,
            "so long and thanks for all the fish".into(),
            Slot::new(42, 0),
            parents.clone(),
            &mut selector,
        );

        block_graph
            .acknowledge_block(hash_2, block_2.clone(), &mut selector, Slot::new(1000, 0))
            .unwrap();

        assert_eq!(block_graph.max_cliques.len(), 2);

        for clique in block_graph.max_cliques {
            if clique.contains(&hash_1) && clique.contains(&hash_2) {
                panic!("two different blocks in the same slot and the same clique")
            }
        }
    }
*/
/*
    #[test]
    fn test_pruning() {
        let cfg = example_consensus_config();
        let mut block_graph = BlockGraph::new(&cfg).unwrap();
        let mut selector = RandomSelector::new(&[0u8; 32].to_vec(), 2, [1u64, 2u64].to_vec())
            .expect("could not initialize selector");

        // everything gonna be ok
        generate_blocks(&mut block_graph, &mut selector, 100_000, (1, 1));
    }
*/
//}
