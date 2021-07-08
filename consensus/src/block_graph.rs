//! All information concerning blocks, the block graph and cliques is managed here.
use super::{config::ConsensusConfig, random_selector::RandomSelector};
use crate::error::ConsensusError;
use crypto::hash::Hash;
use crypto::signature::SignatureEngine;
use models::{Block, BlockHeader, BlockHeaderContent, SerializationContext, Slot};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map, BTreeSet, HashMap, HashSet};
use std::mem;

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
    is_final: bool,
}

impl ActiveBlock {
    /// Computes the fitness of the block
    fn fitness(&self) -> u64 {
        /*
        self.block
            .header
            .endorsements
            .iter()
            .fold(1, |acc, endorsement| match endorsement {
                Some(_) => acc + 1,
                None => acc,
            })
        */
        1
    }
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
        sequence_number: u64,
    },
    Active(ActiveBlock),
    Discarded {
        header: BlockHeader,
        reason: DiscardReason,
        sequence_number: u64,
    },
}

/// The block version that can be exported.
/// Note that the detailed list of operation is not exported
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportCompiledBlock {
    /// Header of the corresponding block.
    pub block: BlockHeader,
    /// For (i, set) in children,
    /// set contains the headers' hashes
    /// of blocks referencing exported block as a parent,
    /// in thread i.
    pub children: Vec<HashSet<Hash>>,
}

#[derive(Debug, Default, Clone)]
pub struct ExportDiscardedBlocks {
    pub map: HashMap<Hash, (DiscardReason, BlockHeader)>,
}

#[derive(Debug, Clone)]
pub struct BlockGraphExport {
    /// Genesis blocks.
    pub genesis_blocks: Vec<Hash>,
    /// Map of active blocks, were blocks are in their exported version.
    pub active_blocks: HashMap<Hash, ExportCompiledBlock>,
    /// Finite cache of discarded blocks, in exported version.
    pub discarded_blocks: ExportDiscardedBlocks,
    /// Best parents hashe in each thread.
    pub best_parents: Vec<Hash>,
    /// Latest final period and block hash in each thread.
    pub latest_final_blocks_periods: Vec<(Hash, u64)>,
    /// Head of the incompatibility graph.
    pub gi_head: HashMap<Hash, HashSet<Hash>>,
    /// List of maximal cliques of compatible blocks.
    pub max_cliques: Vec<HashSet<Hash>>,
}

impl<'a> From<&'a BlockGraph> for BlockGraphExport {
    /// Conversion from blockgraph.
    fn from(block_graph: &'a BlockGraph) -> Self {
        let mut export = BlockGraphExport {
            genesis_blocks: block_graph.genesis_hashes.clone(),
            active_blocks: Default::default(),
            discarded_blocks: Default::default(),
            best_parents: block_graph.best_parents.clone(),
            latest_final_blocks_periods: block_graph.latest_final_blocks_periods.clone(),
            gi_head: block_graph.gi_head.clone(),
            max_cliques: block_graph.max_cliques.clone(),
        };

        for (hash, block) in block_graph.block_statuses.iter() {
            match block {
                BlockStatus::Discarded { header, reason, .. } => {
                    export
                        .discarded_blocks
                        .map
                        .insert(hash.clone(), (reason.clone(), header.clone()));
                }
                BlockStatus::Active(block) => {
                    export.active_blocks.insert(
                        hash.clone(),
                        ExportCompiledBlock {
                            block: block.block.header.clone(),
                            children: block
                                .children
                                .iter()
                                .map(|thread| {
                                    thread
                                        .keys()
                                        .map(|hash| hash.clone())
                                        .collect::<HashSet<Hash>>()
                                })
                                .collect(),
                        },
                    );
                }
                _ => continue,
            }
        }

        export
    }
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
    max_cliques: Vec<HashSet<Hash>>,
    to_propagate: HashMap<Hash, Block>,
    attack_attempts: Vec<Hash>,
}

#[derive(Debug)]
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
            max_cliques: vec![HashSet::new()],
            to_propagate: Default::default(),
            attack_attempts: Default::default(),
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
                BlockStatus::WaitingForDependencies { .. } => {
                    // promote in dependencies
                    self.promote_dep_tree(hash)?;
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
                    ..
                } => {
                    // promote to full block and satisfy self-dependency
                    if unsatisfied_dependencies.remove(&hash) {
                        // a dependency was satisfied: process
                        to_ack.insert((block.header.content.slot, hash));
                    }
                    *header_or_block = HeaderOrBlock::Block(block);
                    // promote in dependencies
                    self.promote_dep_tree(hash)?;
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
                                sequence_number: BlockGraph::new_sequence_number(
                                    &mut self.sequence_counter,
                                ),
                            },
                        );
                        self.promote_dep_tree(hash)?;
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
                                sequence_number: BlockGraph::new_sequence_number(
                                    &mut self.sequence_counter,
                                ),
                            },
                        );
                        self.promote_dep_tree(hash)?;
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
                        self.maybe_note_attack_attempt(&reason, &hash);

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
                                sequence_number: BlockGraph::new_sequence_number(
                                    &mut self.sequence_counter,
                                ),
                            },
                        );
                        self.promote_dep_tree(hash)?;
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
                        self.maybe_note_attack_attempt(&reason, &hash);

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
            self.to_propagate.insert(hash.clone(), active.block.clone());
            for (itm_hash, itm_status) in self.block_statuses.iter_mut() {
                if let BlockStatus::WaitingForDependencies {
                    header_or_block,
                    unsatisfied_dependencies,
                    ..
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

    /// Note an attack attempt if the discard reason indicates one.
    fn maybe_note_attack_attempt(&mut self, reason: &DiscardReason, hash: &Hash) {
        // If invalid, note the attack attempt.
        match reason {
            &DiscardReason::Invalid => {
                self.attack_attempts.push(hash.clone());
            }
            _ => {}
        }
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
            return Ok(CheckOutcome::Discard(DiscardReason::Stale));
        }

        // check if block slot is too much in the future
        if let Some(cur_slot) = current_slot {
            if header.content.slot.period
                > cur_slot
                    .period
                    .saturating_add(self.cfg.future_block_processing_max_periods)
            {
                return Ok(CheckOutcome::WaitForSlot);
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
                Some(BlockStatus::Discarded { reason, .. }) => {
                    // parent is discarded
                    return Ok(CheckOutcome::Discard(*reason));
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
                        return Ok(CheckOutcome::Discard(DiscardReason::Stale));
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
                        Some(BlockStatus::Discarded { reason, .. }) => {
                            return Ok(CheckOutcome::Discard(*reason));
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
                                return Ok(CheckOutcome::Discard(DiscardReason::Stale));
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
        let deps;
        let incomp;
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
                .filter(|c| incomp.is_disjoint(c))
                .for_each(|c| drop(c.insert(hash)));
        } else {
            // fully recompute max cliques
            self.max_cliques = self.compute_max_cliques();
        }

        // compute clique fitnesses and find blockclique
        // note: clique_fitnesses is pair (fitness, -hash_sum) where the second parameter is negative for sorting
        let mut clique_fitnesses = vec![(0u64, num::BigInt::default()); self.max_cliques.len()];
        let mut blockclique_i = 0usize;
        for (clique_i, clique) in self.max_cliques.iter().enumerate() {
            let mut sum_fit: u64 = 0;
            let mut sum_hash = num::BigInt::default();
            for block_h in clique.iter() {
                sum_fit = sum_fit
                    .checked_add(
                        BlockGraph::get_full_active_block(&self.block_statuses, *block_h)
                            .ok_or(ConsensusError::ContainerInconsistency)?
                            .fitness(),
                    )
                    .ok_or(ConsensusError::FitnessOverflow)?;
                sum_hash -=
                    num::BigInt::from_bytes_be(num::bigint::Sign::Plus, &block_h.to_bytes());
            }
            clique_fitnesses[clique_i] = (sum_fit, sum_hash);
            if clique_fitnesses[clique_i] > clique_fitnesses[blockclique_i] {
                blockclique_i = clique_i;
            }
        }

        // update best parents
        {
            let blockclique = &self.max_cliques[blockclique_i];
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
            let fitnesss_threshold = clique_fitnesses[blockclique_i]
                .0
                .saturating_sub(self.cfg.delta_f0);
            // iterate from largest to smallest to minimize reallocations
            let mut indices: Vec<usize> = (0..self.max_cliques.len()).collect();
            indices.sort_unstable_by_key(|&i| std::cmp::Reverse(self.max_cliques[i].len()));
            let mut high_set: HashSet<Hash> = HashSet::new();
            let mut low_set: HashSet<Hash> = HashSet::new();
            let mut keep_mask = vec![true; self.max_cliques.len()];
            for clique_i in indices.into_iter() {
                if clique_fitnesses[clique_i].0 >= fitnesss_threshold {
                    high_set.extend(&self.max_cliques[clique_i]);
                } else {
                    low_set.extend(&self.max_cliques[clique_i]);
                    keep_mask[clique_i] = false;
                }
            }
            let mut clique_i = 0;
            self.max_cliques.retain(|_| {
                clique_i += 1;
                keep_mask[clique_i - 1]
            });
            clique_i = 0;
            clique_fitnesses.retain(|_| {
                clique_i += 1;
                if keep_mask[clique_i - 1] {
                    true
                } else {
                    if blockclique_i > clique_i - 1 {
                        blockclique_i -= 1;
                    }
                    false
                }
            });
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
                    .for_each(|c| drop(c.remove(&stale_block_hash)));
                self.max_cliques.retain(|c| !c.is_empty()); // remove empty cliques
                if self.max_cliques.is_empty() {
                    // make sure at least one clique remains
                    self.max_cliques = vec![HashSet::new()];
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
            //  short-circuiting intersection of cliques from smallest to largest
            let mut indices: Vec<usize> = (0..self.max_cliques.len()).collect();
            indices.sort_unstable_by_key(|&i| self.max_cliques[i].len());
            let mut final_candidates = self.max_cliques[indices[0]].clone();
            for i in 1..indices.len() {
                final_candidates.retain(|v| self.max_cliques[i].contains(v));
                if final_candidates.is_empty() {
                    break;
                }
            }
            // restrict search to cliques with high enough fitness
            indices.retain(|&i| clique_fitnesses[i].0 > self.cfg.delta_f0);
            indices.sort_unstable_by_key(|&i| clique_fitnesses[i].0);
            let mut final_blocks: HashSet<Hash> = HashSet::new();
            for clique_i in indices.into_iter().rev() {
                // check in cliques from highest to lowest fitness
                if final_candidates.is_empty() {
                    // no more final candidates
                    break;
                }
                let clique = &self.max_cliques[clique_i];
                // sum the descendence fitness for each candidate
                let cloned_candidates = final_candidates.clone();
                for candidate_h in cloned_candidates.into_iter() {
                    let mut visited: HashSet<Hash> = HashSet::new();
                    let mut stack: Vec<Hash> = vec![candidate_h];
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
                            if let Some(v) = desc_rem_fitness.checked_sub(b.fitness()) {
                                desc_rem_fitness = v;
                            } else {
                                // block is final
                                final_candidates.remove(&candidate_h);
                                final_blocks.insert(candidate_h);
                                break;
                            }
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
                .for_each(|c| drop(c.remove(&final_block_hash)));
            self.max_cliques.retain(|c| !c.is_empty()); // remove empty cliques
            if self.max_cliques.is_empty() {
                // make sure at least one clique remains
                self.max_cliques = vec![HashSet::new()];
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
    fn prune_active(&mut self) -> Result<HashMap<Hash, Block>, ConsensusError> {
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
                let mut cursor = self.latest_final_blocks_periods[thread as usize].0; // hash of tha latest final in that thread
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

    fn promote_dep_tree(&mut self, hash: Hash) -> Result<(), ConsensusError> {
        let mut to_explore = vec![hash];
        let mut to_promote: HashMap<Hash, (Slot, u64)> = HashMap::new();
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

        let mut to_promote: Vec<(Slot, u64, Hash)> = to_promote
            .into_iter()
            .map(|(h, (slot, seq))| (slot, seq, h))
            .collect();
        to_promote.sort_unstable(); // last ones should have the highest seq number
        for (_slot, _seq, h) in to_promote.into_iter() {
            if let Some(BlockStatus::WaitingForDependencies {
                sequence_number, ..
            }) = self.block_statuses.get_mut(&h)
            {
                *sequence_number = BlockGraph::new_sequence_number(&mut self.sequence_counter);
            }
        }
        Ok(())
    }

    fn prune_waiting_for_dependencies(&mut self) -> Result<(), ConsensusError> {
        let mut to_discard: HashMap<Hash, Option<DiscardReason>> = HashMap::new();
        let mut to_keep: HashMap<Hash, (u64, Slot)> = HashMap::new();

        // list items that are older than the latest final blocks in their threads or have deps that are discarded
        {
            for (hash, block_status) in self.block_statuses.iter() {
                if let BlockStatus::WaitingForDependencies {
                    header_or_block,
                    unsatisfied_dependencies,
                    sequence_number,
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
                    to_keep.insert(*hash, (*sequence_number, header_or_block.get_slot()));
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

    fn prune_slot_waiting(&mut self) {
        let mut slot_waiting: Vec<(Slot, Hash)> = self
            .block_statuses
            .iter()
            .filter_map(|(hash, block_status)| {
                if let BlockStatus::WaitingForSlot(header_or_block) = block_status {
                    return Some((header_or_block.get_slot(), *hash));
                }
                None
            })
            .collect();
        slot_waiting.sort_unstable();
        let retained: HashSet<Hash> = slot_waiting
            .into_iter()
            .take(self.cfg.max_future_processing_blocks)
            .map(|(_slot, hash)| hash)
            .collect();
        self.block_statuses.retain(|hash, block_status| {
            if let BlockStatus::WaitingForSlot(_) = block_status {
                return retained.contains(hash);
            }
            true
        });
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
    pub fn prune(&mut self) -> Result<HashMap<Hash, Block>, ConsensusError> {
        // Step 1: discard final blocks that are not useful to the graph anymore and return them
        let discarded_finals = self.prune_active()?;

        // Step 2: prune slot waiting blocks
        self.prune_slot_waiting();

        // Step 3: prune dependency waiting blocks
        self.prune_waiting_for_dependencies()?;

        // Step 4: prune discarded
        self.prune_discarded()?;

        Ok(discarded_finals)
    }

    // get the current block wishlist
    pub fn get_block_wishlist(&self) -> Result<HashSet<Hash>, ConsensusError> {
        let mut wishlist = HashSet::new();
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
                    wishlist.insert(*unsatisfied_h);
                }
            }
        }
        Ok(wishlist)
    }

    // Get the headers to be propagated.
    // Must be called by the consensus worker within `block_db_changed`.
    pub fn get_blocks_to_propagate(&mut self) -> HashMap<Hash, Block> {
        mem::take(&mut self.to_propagate)
    }

    // Get the hashes of objects that were attack attempts.
    // Must be called by the consensus worker within `block_db_changed`.
    pub fn get_attack_attempts(&mut self) -> Vec<Hash> {
        mem::take(&mut self.attack_attempts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::UTime;

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
}
