// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! All information concerning blocks, the block graph and cliques is managed here.
use crate::{
    bootstrapable_graph::BootstrapableGraph,
    error::{GraphError, GraphResult as Result},
    export_active_block::ExportActiveBlock,
    settings::GraphConfig,
};
use massa_hash::Hash;
use massa_logging::massa_trace;
use massa_models::{
    active_block::ActiveBlock, api::BlockGraphStatus, clique::Clique, wrapped::WrappedContent,
    WrappedBlock,
};
use massa_models::{
    prehash::{BuildHashMapper, PreHashMap, PreHashSet},
    WrappedHeader,
};
use massa_models::{
    Address, Block, BlockHeader, BlockHeaderSerializer, BlockId, BlockSerializer, Slot,
};
use massa_pos_exports::SelectorController;
use massa_signature::PublicKey;
use massa_storage::Storage;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map, BTreeSet, HashMap, VecDeque};
use std::mem;
use tracing::{debug, info};

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum HeaderOrBlock {
    Header(WrappedHeader),
    Block {
        id: BlockId,
        slot: Slot,
        storage: Storage,
    },
}

impl HeaderOrBlock {
    /// Gets slot for that header or block
    pub fn get_slot(&self) -> Slot {
        match self {
            HeaderOrBlock::Header(header) => header.content.slot,
            HeaderOrBlock::Block { slot, .. } => *slot,
        }
    }
}

/// Something can be discarded
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscardReason {
    /// Block is invalid, either structurally, or because of some incompatibility. The String contains the reason for info or debugging.
    Invalid(String),
    /// Block is incompatible with a final block.
    Stale,
    /// Block has enough fitness.
    Final,
}

/// Enum used in `BlockGraph`'s state machine
#[derive(Debug, Clone)]
enum BlockStatus {
    /// The block/header has reached consensus but no consensus-level check has been performed.
    /// It will be processed during the next iteration
    Incoming(HeaderOrBlock),
    /// The block's or header's slot is too much in the future.
    /// It will be processed at the block/header slot
    WaitingForSlot(HeaderOrBlock),
    /// The block references an unknown Block id
    WaitingForDependencies {
        /// Given header/block
        header_or_block: HeaderOrBlock,
        /// includes self if it's only a header
        unsatisfied_dependencies: PreHashSet<BlockId>,
        /// Used to limit and sort the number of blocks/headers waiting for dependencies
        sequence_number: u64,
    },
    /// The block was checked and included in the blockgraph
    Active {
        a_block: Box<ActiveBlock>,
        storage: Storage,
    },
    /// The block was discarded and is kept to avoid reprocessing it
    Discarded {
        /// Just the slot of that block
        slot: Slot,
        /// Address of the creator of the block
        creator: Address,
        /// Ids of parents blocks
        parents: Vec<BlockId>,
        /// why it was discarded
        reason: DiscardReason,
        /// Used to limit and sort the number of blocks/headers waiting for dependencies
        sequence_number: u64,
    },
}

/// Block status in the graph that can be exported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExportBlockStatus {
    /// received but not yet graph processed
    Incoming,
    /// waiting for its slot
    WaitingForSlot,
    /// waiting for a missing dependency
    WaitingForDependencies,
    /// valid and not yet final
    Active(Block),
    /// immutable
    Final(Block),
    /// not part of the graph
    Discarded(DiscardReason),
}

/// The block version that can be exported.
/// Note that the detailed list of operation is not exported
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportCompiledBlock {
    /// Header of the corresponding block.
    pub header: WrappedHeader,
    /// For (i, set) in children,
    /// set contains the headers' hashes
    /// of blocks referencing exported block as a parent,
    /// in thread i.
    pub children: Vec<PreHashSet<BlockId>>,
    /// Active or final
    pub is_final: bool,
}

/// Status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    /// without enough fitness to be part of immutable history
    Active,
    /// with enough fitness to be part of immutable history
    Final,
}

impl<'a> BlockGraphExport {
    /// Conversion from blockgraph.
    pub fn extract_from(
        block_graph: &'a BlockGraph,
        slot_start: Option<Slot>,
        slot_end: Option<Slot>,
    ) -> Result<Self> {
        let mut export = BlockGraphExport {
            genesis_blocks: block_graph.genesis_hashes.clone(),
            active_blocks: PreHashMap::with_capacity(block_graph.block_statuses.len()),
            discarded_blocks: PreHashMap::with_capacity(block_graph.block_statuses.len()),
            best_parents: block_graph.best_parents.clone(),
            latest_final_blocks_periods: block_graph.latest_final_blocks_periods.clone(),
            gi_head: block_graph.gi_head.clone(),
            max_cliques: block_graph.max_cliques.clone(),
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

        for (hash, block) in block_graph.block_statuses.iter() {
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
                                GraphError::MissingBlock(format!(
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
}

/// Bootstrap compatible version of the block graph
#[derive(Debug, Clone)]
#[allow(clippy::type_complexity)]
pub struct BlockGraphExport {
    /// Genesis blocks.
    pub genesis_blocks: Vec<BlockId>,
    /// Map of active blocks, were blocks are in their exported version.
    pub active_blocks: PreHashMap<BlockId, ExportCompiledBlock>,
    /// Finite cache of discarded blocks, in exported version (slot, creator_address, parents).
    pub discarded_blocks: PreHashMap<BlockId, (DiscardReason, (Slot, Address, Vec<BlockId>))>,
    /// Best parents hashes in each thread.
    pub best_parents: Vec<(BlockId, u64)>,
    /// Latest final period and block hash in each thread.
    pub latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// Head of the incompatibility graph.
    pub gi_head: PreHashMap<BlockId, PreHashSet<BlockId>>,
    /// List of maximal cliques of compatible blocks.
    pub max_cliques: Vec<Clique>,
}

/// Graph management
pub struct BlockGraph {
    /// Consensus related configuration
    cfg: GraphConfig,
    /// Block ids of genesis blocks
    genesis_hashes: Vec<BlockId>,
    /// Used to limit the number of waiting and discarded blocks
    sequence_counter: u64,
    /// Every block we know about
    block_statuses: PreHashMap<BlockId, BlockStatus>,
    /// Ids of incoming blocks/headers
    incoming_index: PreHashSet<BlockId>,
    /// ids of waiting for slot blocks/headers
    waiting_for_slot_index: PreHashSet<BlockId>,
    /// ids of waiting for dependencies blocks/headers
    waiting_for_dependencies_index: PreHashSet<BlockId>,
    /// ids of active blocks
    active_index: PreHashSet<BlockId>,
    /// ids of discarded blocks
    discarded_index: PreHashSet<BlockId>,
    /// One (block id, period) per thread
    latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// One `(block id, period)` per thread TODO not sure I understand the difference with `latest_final_blocks_periods`
    best_parents: Vec<(BlockId, u64)>,
    /// Incompatibility graph: maps a block id to the block ids it is incompatible with
    /// One entry per Active Block
    gi_head: PreHashMap<BlockId, PreHashSet<BlockId>>,
    /// All the cliques
    max_cliques: Vec<Clique>,
    /// Blocks that need to be propagated
    to_propagate: PreHashMap<BlockId, Storage>,
    /// List of block ids we think are attack attempts
    attack_attempts: Vec<BlockId>,
    /// Newly final blocks
    new_final_blocks: PreHashSet<BlockId>,
    /// Newly stale block mapped to creator and slot
    new_stale_blocks: PreHashMap<BlockId, (Address, Slot)>,
    /// Shared storage,
    storage: Storage,
    /// Selector controller
    selector_controller: Box<dyn SelectorController>,
}

/// Possible output of a header check
#[derive(Debug)]
enum HeaderCheckOutcome {
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
enum EndorsementsCheckOutcome {
    /// Everything is ok
    Proceed,
    /// There is something wrong with that endorsement
    Discard(DiscardReason),
    /// It must wait for its slot to be fully processed
    WaitForSlot,
}

/// Creates genesis block in given thread.
///
/// # Arguments
/// * `cfg`: consensus configuration
/// * `thread_number`: thread in which we want a genesis block
pub fn create_genesis_block(
    cfg: &GraphConfig,
    thread_number: u8,
) -> Result<(BlockId, WrappedBlock)> {
    let keypair = &cfg.genesis_key;
    let header = BlockHeader::new_wrapped(
        BlockHeader {
            slot: Slot::new(0, thread_number),
            parents: Vec::new(),
            operation_merkle_root: Hash::compute_from(&Vec::new()),
            endorsements: Vec::new(),
        },
        BlockHeaderSerializer::new(),
        keypair,
    )?;

    Ok((
        header.id,
        Block::new_wrapped(
            Block {
                header,
                operations: Default::default(),
            },
            BlockSerializer::new(),
            keypair,
        )?,
    ))
}

impl BlockGraph {
    /// Creates a new `BlockGraph`.
    ///
    /// # Argument
    /// * `cfg`: consensus configuration.
    /// * `init`: A bootstrap graph to start the graph with
    /// * `storage`: A shared storage that share data across all modules.
    /// * `selector_controller`: Access to the PoS selector to get draws
    pub async fn new(
        cfg: GraphConfig,
        init: Option<BootstrapableGraph>,
        storage: Storage,
        selector_controller: Box<dyn SelectorController>,
    ) -> Result<Self> {
        // load genesis blocks

        let mut block_statuses = PreHashMap::default();
        let mut genesis_block_ids = Vec::with_capacity(cfg.thread_count as usize);
        for thread in 0u8..cfg.thread_count {
            let (block_id, block) = create_genesis_block(&cfg, thread).map_err(|err| {
                GraphError::GenesisCreationError(format!("genesis error {}", err))
            })?;
            let mut storage = storage.clone_without_refs();
            storage.store_block(block.clone());
            genesis_block_ids.push(block_id);
            block_statuses.insert(
                block_id,
                BlockStatus::Active {
                    a_block: Box::new(ActiveBlock {
                        creator_address: block.creator_address,
                        parents: Vec::new(),
                        children: vec![PreHashMap::default(); cfg.thread_count as usize],
                        descendants: Default::default(),
                        is_final: true,
                        block_id,
                        slot: block.content.header.content.slot,
                        fitness: block.get_fitness(),
                    }),
                    storage,
                },
            );
        }

        massa_trace!("consensus.block_graph.new", {});
        if let Some(BootstrapableGraph { final_blocks }) = init {
            // load final blocks
            let final_blocks: Vec<(ActiveBlock, Storage)> = final_blocks
                .into_iter()
                .map(|export_b| export_b.to_active_block(&storage, cfg.thread_count))
                .collect::<Result<_, GraphError>>()?;

            // compute latest_final_blocks_periods
            let mut latest_final_blocks_periods: Vec<(BlockId, u64)> =
                genesis_block_ids.iter().map(|id| (*id, 0u64)).collect();
            for (b, _) in &final_blocks {
                if let Some(v) = latest_final_blocks_periods.get_mut(b.slot.thread as usize) {
                    if b.slot.period > v.1 {
                        *v = (b.block_id, b.slot.period);
                    }
                }
            }

            // generate graph
            let mut res_graph = BlockGraph {
                cfg: cfg.clone(),
                sequence_counter: 0,
                genesis_hashes: genesis_block_ids,
                active_index: final_blocks.iter().map(|(b, _)| b.block_id).collect(),
                incoming_index: Default::default(),
                waiting_for_slot_index: Default::default(),
                waiting_for_dependencies_index: Default::default(),
                discarded_index: Default::default(),
                best_parents: latest_final_blocks_periods.clone(),
                latest_final_blocks_periods,
                gi_head: Default::default(),
                max_cliques: vec![Clique {
                    block_ids: PreHashSet::<BlockId>::default(),
                    fitness: 0,
                    is_blockclique: true,
                }],
                to_propagate: Default::default(),
                attack_attempts: Default::default(),
                new_final_blocks: Default::default(),
                new_stale_blocks: Default::default(),
                storage,
                selector_controller,
                block_statuses: final_blocks
                    .into_iter()
                    .map(|(b, s)| {
                        Ok((
                            b.block_id,
                            BlockStatus::Active {
                                a_block: Box::new(b),
                                storage: s,
                            },
                        ))
                    })
                    .collect::<Result<_>>()?,
            };

            // claim parent refs
            for (_b_id, block_status) in res_graph.block_statuses.iter_mut() {
                if let BlockStatus::Active {
                    a_block,
                    storage: block_storage,
                } = block_status
                {
                    // claim parent refs
                    let n_claimed_parents = block_storage
                        .claim_block_refs(&a_block.parents.iter().map(|(p_id, _)| *p_id).collect())
                        .len();

                    if !a_block.is_final {
                        // note: parents of final blocks will be missing, that's ok, but it shouldn't be the case for non-finals
                        if n_claimed_parents != cfg.thread_count as usize {
                            return Err(GraphError::MissingBlock(
                                "block storage could not claim refs to all parent blocks".into(),
                            ));
                        }
                    }
                }
            }

            // list active block parents
            let active_blocks_map: PreHashMap<BlockId, (Slot, Vec<BlockId>)> = res_graph
                .block_statuses
                .iter()
                .filter_map(|(h, s)| {
                    if let BlockStatus::Active { a_block: a, .. } = s {
                        return Some((*h, (a.slot, a.parents.iter().map(|(ph, _)| *ph).collect())));
                    }
                    None
                })
                .collect();
            // deduce children and descendants
            for (b_id, (b_slot, b_parents)) in active_blocks_map.into_iter() {
                // deduce children
                for parent_id in &b_parents {
                    if let Some(BlockStatus::Active {
                        a_block: parent, ..
                    }) = res_graph.block_statuses.get_mut(parent_id)
                    {
                        parent.children[b_slot.thread as usize].insert(b_id, b_slot.period);
                    }
                }

                // deduce descendants
                let mut ancestors: VecDeque<BlockId> = b_parents.into_iter().collect();
                let mut visited: PreHashSet<BlockId> = Default::default();
                while let Some(ancestor_h) = ancestors.pop_back() {
                    if !visited.insert(ancestor_h) {
                        continue;
                    }
                    if let Some(BlockStatus::Active { a_block: ab, .. }) =
                        res_graph.block_statuses.get_mut(&ancestor_h)
                    {
                        ab.descendants.insert(b_id);
                        for (ancestor_parent_h, _) in ab.parents.iter() {
                            ancestors.push_front(*ancestor_parent_h);
                        }
                    }
                }
            }
            Ok(res_graph)
        } else {
            Ok(BlockGraph {
                cfg,
                sequence_counter: 0,
                block_statuses,
                incoming_index: Default::default(),
                waiting_for_slot_index: Default::default(),
                waiting_for_dependencies_index: Default::default(),
                active_index: genesis_block_ids.iter().copied().collect(),
                discarded_index: Default::default(),
                latest_final_blocks_periods: genesis_block_ids.iter().map(|h| (*h, 0)).collect(),
                best_parents: genesis_block_ids.iter().map(|v| (*v, 0)).collect(),
                genesis_hashes: genesis_block_ids,
                gi_head: PreHashMap::default(),
                max_cliques: vec![Clique {
                    block_ids: PreHashSet::<BlockId>::default(),
                    fitness: 0,
                    is_blockclique: true,
                }],
                to_propagate: Default::default(),
                attack_attempts: Default::default(),
                new_final_blocks: Default::default(),
                new_stale_blocks: Default::default(),
                storage,
                selector_controller,
            })
        }
    }

    /// export full graph in a bootstrap compatible version
    pub fn export_bootstrap_graph(&self) -> Result<BootstrapableGraph> {
        let mut required_final_blocks: PreHashSet<_> = self.list_required_active_blocks()?;
        required_final_blocks.retain(|b_id| {
            if let Some(BlockStatus::Active { a_block, .. }) = self.block_statuses.get(b_id) {
                if a_block.is_final {
                    // filter only final actives
                    return true;
                }
            }
            false
        });
        let mut final_blocks: Vec<ExportActiveBlock> =
            Vec::with_capacity(required_final_blocks.len());
        for b_id in &required_final_blocks {
            if let Some(BlockStatus::Active { a_block, storage }) = self.block_statuses.get(b_id) {
                final_blocks.push(ExportActiveBlock::from_active_block(a_block, storage));
            } else {
                return Err(GraphError::ContainerInconsistency(format!(
                    "block {} was expected to be active but wasn't on bootstrap graph export",
                    b_id
                )));
            }
        }

        Ok(BootstrapableGraph { final_blocks })
    }

    /// Gets latest final blocks (hash, period) for each thread.
    pub fn get_latest_final_blocks_periods(&self) -> &Vec<(BlockId, u64)> {
        &self.latest_final_blocks_periods
    }

    /// Gets best parents.
    pub fn get_best_parents(&self) -> &Vec<(BlockId, u64)> {
        &self.best_parents
    }

    /// Gets the list of cliques.
    pub fn get_cliques(&self) -> Vec<Clique> {
        self.max_cliques.clone()
    }

    /// Returns the list of block IDs created by a given address, and their finality statuses
    pub fn get_block_ids_by_creator(&self, address: &Address) -> PreHashMap<BlockId, Status> {
        // iterate on active (final and non-final) blocks
        self.active_index
            .iter()
            .filter_map(|block_id| match self.block_statuses.get(block_id) {
                Some(BlockStatus::Active { a_block, .. }) => {
                    if a_block.creator_address == *address {
                        Some((
                            *block_id,
                            if a_block.is_final {
                                Status::Final
                            } else {
                                Status::Active
                            },
                        ))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect()
    }

    /// Gets whole compiled block corresponding to given hash, if it is active.
    ///
    /// # Argument
    /// * `block_id`: block ID
    pub fn get_active_block(&self, block_id: &BlockId) -> Option<(&ActiveBlock, &Storage)> {
        BlockGraph::get_full_active_block(&self.block_statuses, *block_id)
    }

    /// get block graph status
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

    /// signal new slot
    pub fn slot_tick(&mut self, current_slot: Option<Slot>) -> Result<()> {
        // list all elements for which the time has come
        let to_process: BTreeSet<(Slot, BlockId)> = self
            .waiting_for_slot_index
            .iter()
            .filter_map(|b_id| match self.block_statuses.get(b_id) {
                Some(BlockStatus::WaitingForSlot(header_or_block)) => {
                    let slot = header_or_block.get_slot();
                    if Some(slot) <= current_slot {
                        Some((slot, *b_id))
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        massa_trace!("consensus.block_graph.slot_tick", {});
        // process those elements
        self.rec_process(to_process, current_slot)?;

        Ok(())
    }

    /// A new header has come !
    ///
    /// Checks performed:
    /// - Ignore genesis blocks.
    /// - See `process`.
    pub fn incoming_header(
        &mut self,
        block_id: BlockId,
        header: WrappedHeader,
        current_slot: Option<Slot>,
    ) -> Result<()> {
        // ignore genesis blocks
        if self.genesis_hashes.contains(&block_id) {
            return Ok(());
        }

        debug!(
            "received header {} for slot {}",
            block_id, header.content.slot
        );
        massa_trace!("consensus.block_graph.incoming_header", {"block_id": block_id, "header": header});
        let mut to_ack: BTreeSet<(Slot, BlockId)> = BTreeSet::new();
        match self.block_statuses.entry(block_id) {
            // if absent => add as Incoming, call rec_ack on it
            hash_map::Entry::Vacant(vac) => {
                to_ack.insert((header.content.slot, block_id));
                vac.insert(BlockStatus::Incoming(HeaderOrBlock::Header(header)));
                self.incoming_index.insert(block_id);
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
                    self.promote_dep_tree(block_id)?;
                }
                _ => {}
            },
        }

        // process
        self.rec_process(to_ack, current_slot)?;

        Ok(())
    }

    /// A new block has come
    ///
    /// Checks performed:
    /// - Ignore genesis blocks.
    /// - See `process`.
    pub fn incoming_block(
        &mut self,
        block_id: BlockId,
        slot: Slot,
        current_slot: Option<Slot>,
        storage: Storage,
    ) -> Result<()> {
        // ignore genesis blocks
        if self.genesis_hashes.contains(&block_id) {
            return Ok(());
        }

        let mut to_ack: BTreeSet<(Slot, BlockId)> = BTreeSet::new();
        match self.block_statuses.entry(block_id) {
            // if absent => add as Incoming, call rec_ack on it
            hash_map::Entry::Vacant(vac) => {
                to_ack.insert((slot, block_id));
                vac.insert(BlockStatus::Incoming(HeaderOrBlock::Block {
                    id: block_id,
                    slot,
                    storage,
                }));
                self.incoming_index.insert(block_id);
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
                    *header_or_block = HeaderOrBlock::Block {
                        id: block_id,
                        slot,
                        storage,
                    };
                }
                BlockStatus::WaitingForDependencies {
                    header_or_block,
                    unsatisfied_dependencies,
                    ..
                } => {
                    // promote to full block and satisfy self-dependency
                    if unsatisfied_dependencies.remove(&block_id) {
                        // a dependency was satisfied: process
                        to_ack.insert((slot, block_id));
                    }
                    *header_or_block = HeaderOrBlock::Block {
                        id: block_id,
                        slot,
                        storage,
                    };
                    // promote in dependencies
                    self.promote_dep_tree(block_id)?;
                }
                _ => return Ok(()),
            },
        }

        // process
        self.rec_process(to_ack, current_slot)?;

        Ok(())
    }

    fn new_sequence_number(sequence_counter: &mut u64) -> u64 {
        let res = *sequence_counter;
        *sequence_counter += 1;
        res
    }

    /// acknowledge a set of items recursively
    fn rec_process(
        &mut self,
        mut to_ack: BTreeSet<(Slot, BlockId)>,
        current_slot: Option<Slot>,
    ) -> Result<()> {
        // order processing by (slot, hash)
        while let Some((_slot, hash)) = to_ack.pop_first() {
            to_ack.extend(self.process(hash, current_slot)?)
        }
        Ok(())
    }

    /// Acknowledge a single item, return a set of items to re-ack
    fn process(
        &mut self,
        block_id: BlockId,
        current_slot: Option<Slot>,
    ) -> Result<BTreeSet<(Slot, BlockId)>> {
        // list items to reprocess
        let mut reprocess = BTreeSet::new();

        massa_trace!("consensus.block_graph.process", { "block_id": block_id });
        // control all the waiting states and try to get a valid block
        let (
            valid_block_creator,
            valid_block_slot,
            valid_block_parents_hash_period,
            valid_block_incomp,
            valid_block_inherited_incomp_count,
            valid_block_storage,
            valid_block_fitness,
        ) = match self.block_statuses.get(&block_id) {
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
                    return Err(GraphError::ContainerInconsistency(format!(
                        "inconsistency inside block statuses removing incoming header {}",
                        block_id
                    )));
                };
                match self.check_header(&block_id, &header, current_slot)? {
                    HeaderCheckOutcome::Proceed { .. } => {
                        // set as waiting dependencies
                        let mut dependencies = PreHashSet::<BlockId>::default();
                        dependencies.insert(block_id); // add self as unsatisfied
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::WaitingForDependencies {
                                header_or_block: HeaderOrBlock::Header(header),
                                unsatisfied_dependencies: dependencies,
                                sequence_number: BlockGraph::new_sequence_number(
                                    &mut self.sequence_counter,
                                ),
                            },
                        );
                        self.waiting_for_dependencies_index.insert(block_id);
                        self.promote_dep_tree(block_id)?;

                        massa_trace!(
                            "consensus.block_graph.process.incoming_header.waiting_for_self",
                            { "block_id": block_id }
                        );
                        return Ok(BTreeSet::new());
                    }
                    HeaderCheckOutcome::WaitForDependencies(mut dependencies) => {
                        // set as waiting dependencies
                        dependencies.insert(block_id); // add self as unsatisfied
                        massa_trace!("consensus.block_graph.process.incoming_header.waiting_for_dependencies", {"block_id": block_id, "dependencies": dependencies});

                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::WaitingForDependencies {
                                header_or_block: HeaderOrBlock::Header(header),
                                unsatisfied_dependencies: dependencies,
                                sequence_number: BlockGraph::new_sequence_number(
                                    &mut self.sequence_counter,
                                ),
                            },
                        );
                        self.waiting_for_dependencies_index.insert(block_id);
                        self.promote_dep_tree(block_id)?;

                        return Ok(BTreeSet::new());
                    }
                    HeaderCheckOutcome::WaitForSlot => {
                        // make it wait for slot
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::WaitingForSlot(HeaderOrBlock::Header(header)),
                        );
                        self.waiting_for_slot_index.insert(block_id);

                        massa_trace!(
                            "consensus.block_graph.process.incoming_header.waiting_for_slot",
                            { "block_id": block_id }
                        );
                        return Ok(BTreeSet::new());
                    }
                    HeaderCheckOutcome::Discard(reason) => {
                        self.maybe_note_attack_attempt(&reason, &block_id);
                        massa_trace!("consensus.block_graph.process.incoming_header.discarded", {"block_id": block_id, "reason": reason});
                        // count stales
                        if reason == DiscardReason::Stale {
                            self.new_stale_blocks
                                .insert(block_id, (header.creator_address, header.content.slot));
                        }
                        // discard
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::Discarded {
                                slot: header.content.slot,
                                creator: header.creator_address,
                                parents: header.content.parents,
                                reason,
                                sequence_number: BlockGraph::new_sequence_number(
                                    &mut self.sequence_counter,
                                ),
                            },
                        );
                        self.discarded_index.insert(block_id);

                        return Ok(BTreeSet::new());
                    }
                }
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
                        return Err(GraphError::ContainerInconsistency(format!(
                            "inconsistency inside block statuses removing incoming block {}",
                            block_id
                        )));
                    };

                let stored_block = storage
                    .read_blocks()
                    .get(&block_id)
                    .cloned()
                    .expect("incoming block not found in storage");

                match self.check_header(&block_id, &stored_block.content.header, current_slot)? {
                    HeaderCheckOutcome::Proceed {
                        parents_hash_period,
                        incompatibilities,
                        inherited_incompatibilities_count,
                        fitness,
                    } => {
                        // block is valid: remove it from Incoming and return it
                        massa_trace!("consensus.block_graph.process.incoming_block.valid", {
                            "block_id": block_id
                        });
                        (
                            stored_block.content.header.creator_public_key,
                            slot,
                            parents_hash_period,
                            incompatibilities,
                            inherited_incompatibilities_count,
                            storage,
                            fitness,
                        )
                    }
                    HeaderCheckOutcome::WaitForDependencies(dependencies) => {
                        // set as waiting dependencies
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::WaitingForDependencies {
                                header_or_block: HeaderOrBlock::Block {
                                    id: block_id,
                                    slot,
                                    storage,
                                },
                                unsatisfied_dependencies: dependencies,
                                sequence_number: BlockGraph::new_sequence_number(
                                    &mut self.sequence_counter,
                                ),
                            },
                        );
                        self.waiting_for_dependencies_index.insert(block_id);
                        self.promote_dep_tree(block_id)?;
                        massa_trace!(
                            "consensus.block_graph.process.incoming_block.waiting_for_dependencies",
                            { "block_id": block_id }
                        );
                        return Ok(BTreeSet::new());
                    }
                    HeaderCheckOutcome::WaitForSlot => {
                        // set as waiting for slot
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::WaitingForSlot(HeaderOrBlock::Block {
                                id: block_id,
                                slot,
                                storage,
                            }),
                        );
                        self.waiting_for_slot_index.insert(block_id);

                        massa_trace!(
                            "consensus.block_graph.process.incoming_block.waiting_for_slot",
                            { "block_id": block_id }
                        );
                        return Ok(BTreeSet::new());
                    }
                    HeaderCheckOutcome::Discard(reason) => {
                        self.maybe_note_attack_attempt(&reason, &block_id);
                        massa_trace!("consensus.block_graph.process.incoming_block.discarded", {"block_id": block_id, "reason": reason});
                        // count stales
                        if reason == DiscardReason::Stale {
                            self.new_stale_blocks.insert(
                                block_id,
                                (
                                    stored_block.content.header.creator_address,
                                    stored_block.content.header.content.slot,
                                ),
                            );
                        }
                        // add to discard
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::Discarded {
                                slot: stored_block.content.header.content.slot,
                                creator: stored_block.creator_address,
                                parents: stored_block.content.header.content.parents.clone(),
                                reason,
                                sequence_number: BlockGraph::new_sequence_number(
                                    &mut self.sequence_counter,
                                ),
                            },
                        );
                        self.discarded_index.insert(block_id);

                        return Ok(BTreeSet::new());
                    }
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
                    return Err(GraphError::ContainerInconsistency(format!("inconsistency inside block statuses removing waiting for slot block or header {}", block_id)));
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
                    return Err(GraphError::ContainerInconsistency(format!("inconsistency inside block statuses removing waiting for slot header or block {}", block_id)));
                }
            }
        };

        // add block to graph
        self.add_block_to_graph(
            block_id,
            valid_block_parents_hash_period,
            valid_block_creator,
            valid_block_slot,
            valid_block_incomp,
            valid_block_inherited_incomp_count,
            valid_block_fitness,
            valid_block_storage,
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

    /// Note an attack attempt if the discard reason indicates one.
    fn maybe_note_attack_attempt(&mut self, reason: &DiscardReason, hash: &BlockId) {
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

    /// Gets whole `ActiveBlock` corresponding to given `block_id`
    ///
    /// # Argument
    /// * `block_id`: block ID
    fn get_full_active_block(
        block_statuses: &PreHashMap<BlockId, BlockStatus>,
        block_id: BlockId,
    ) -> Option<(&ActiveBlock, &Storage)> {
        match block_statuses.get(&block_id) {
            Some(BlockStatus::Active { a_block, storage }) => Some((a_block.as_ref(), storage)),
            _ => None,
        }
    }

    /// Gets a block and all its descendants
    ///
    /// # Argument
    /// * hash : hash of the given block
    fn get_active_block_and_descendants(&self, block_id: &BlockId) -> Result<PreHashSet<BlockId>> {
        let mut to_visit = vec![*block_id];
        let mut result = PreHashSet::<BlockId>::default();
        while let Some(visit_h) = to_visit.pop() {
            if !result.insert(visit_h) {
                continue; // already visited
            }
            BlockGraph::get_full_active_block(&self.block_statuses, visit_h)
                .ok_or_else(|| GraphError::ContainerInconsistency(format!("inconsistency inside block statuses iterating through descendants of {} - missing {}", block_id, visit_h)))?
                .0
                .children
                .iter()
                .for_each(|thread_children| to_visit.extend(thread_children.keys()));
        }
        Ok(result)
    }

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
    fn check_header(
        &self,
        block_id: &BlockId,
        header: &WrappedHeader,
        current_slot: Option<Slot>,
    ) -> Result<HeaderCheckOutcome> {
        massa_trace!("consensus.block_graph.check_header", {
            "block_id": block_id
        });
        let mut parents: Vec<(BlockId, u64)> = Vec::with_capacity(self.cfg.thread_count as usize);
        let mut incomp = PreHashSet::<BlockId>::default();
        let mut missing_deps = PreHashSet::<BlockId>::default();
        let creator_addr = header.creator_address;

        // check that is older than the latest final block in that thread
        // Note: this excludes genesis blocks
        if header.content.slot.period
            <= self.latest_final_blocks_periods[header.content.slot.thread as usize].1
        {
            return Ok(HeaderCheckOutcome::Discard(DiscardReason::Stale));
        }

        // check if block slot is too much in the future
        if let Some(cur_slot) = current_slot {
            if header.content.slot.period
                > cur_slot
                    .period
                    .saturating_add(self.cfg.future_block_processing_max_periods)
            {
                return Ok(HeaderCheckOutcome::WaitForSlot);
            }
        }

        // check if it was the creator's turn to create this block
        // (step 1 in consensus/pos.md)
        let slot_draw_address = match self.selector_controller.get_producer(header.content.slot) {
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
        for parent_thread in 0u8..self.cfg.thread_count {
            let parent_hash = header.content.parents[parent_thread as usize];
            match self.block_statuses.get(&parent_hash) {
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
                    if let Some(p_incomp) = self.gi_head.get(&parent_hash) {
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
                    if self.genesis_hashes.contains(&parent_hash) {
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
            let mut gp_max_slots = vec![0u64; self.cfg.thread_count as usize];
            for parent_i in 0..self.cfg.thread_count {
                let (parent_h, parent_period) = parents[parent_i as usize];
                let parent = self
                    .get_active_block(&parent_h)
                    .ok_or_else(|| {
                        GraphError::ContainerInconsistency(format!(
                            "inconsistency inside block statuses searching parent {} of block {}",
                            parent_h, block_id
                        ))
                    })?
                    .0;
                if parent_period < gp_max_slots[parent_i as usize] {
                    // a parent is earlier than a block known by another parent in that thread
                    return Ok(HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                        "a parent is earlier than a block known by another parent in that thread"
                            .to_string(),
                    )));
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
                    let gp_h = parent.parents[gp_i as usize].0;
                    match self.block_statuses.get(&gp_h) {
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
        let parent_in_own_thread = BlockGraph::get_full_active_block(
            &self.block_statuses,
            parents[header.content.slot.thread as usize].0,
        )
        .ok_or_else(|| {
            GraphError::ContainerInconsistency(format!(
                "inconsistency inside block statuses searching parent {} in own thread of block {}",
                parents[header.content.slot.thread as usize].0, block_id
            ))
        })?
        .0;

        // check endorsements
        match self.check_endorsements(header, parent_in_own_thread)? {
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
                Result::<()>::Ok(())
            })?;

        // grandpa incompatibility test
        for tau in (0u8..self.cfg.thread_count).filter(|&t| t != header.content.slot.thread) {
            // for each parent in a different thread tau
            // traverse parent's descendants in tau
            let mut to_explore = vec![(0usize, header.content.parents[tau as usize])];
            while let Some((cur_gen, cur_h)) = to_explore.pop() {
                let cur_b = BlockGraph::get_full_active_block(&self.block_statuses, cur_h)
                    .ok_or_else(|| GraphError::ContainerInconsistency(format!("inconsistency inside block statuses searching {} while checking grandpa incompatibility of block {}",cur_h,  block_id)))?.0;

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
                            GraphError::MissingBlock(format!(
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
                if BlockGraph::get_full_active_block(
                    &self.block_statuses,
                    parent_id,
                )
                .ok_or_else(||
                    GraphError::ContainerInconsistency(
                        format!("inconsistency inside block statuses searching {} check if the parent in tauB has a strictly lower period number than B's parent in tauB while checking grandpa incompatibility of block {}",
                        parent_id,
                        block_id)
                    ))?
                .0
                .slot
                .period
                    < parent_in_own_thread.slot.period
                {
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
            &self
                .active_index
                .iter()
                .filter_map(|h| {
                    if let Some(BlockStatus::Active { a_block: a, .. }) = self.block_statuses.get(h)
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
    fn check_endorsements(
        &self,
        header: &WrappedHeader,
        parent_in_own_thread: &ActiveBlock,
    ) -> Result<EndorsementsCheckOutcome> {
        // check endorsements
        let endorsement_draws = match self
            .selector_controller
            .get_selection(parent_in_own_thread.slot)
        {
            Ok(sel) => sel.endorsements,
            Err(_) => return Ok(EndorsementsCheckOutcome::WaitForSlot),
        };
        for endorsement in header.content.endorsements.iter() {
            // check that the draw is correct
            if endorsement.creator_address != endorsement_draws[endorsement.content.index as usize]
            {
                return Ok(EndorsementsCheckOutcome::Discard(DiscardReason::Invalid(
                    format!(
                        "endorser draw mismatch for header in slot: {}",
                        header.content.slot
                    ),
                )));
            }
            // check that the endorsement slot matches the endorsed block
            if endorsement.content.slot != parent_in_own_thread.slot {
                return Ok(EndorsementsCheckOutcome::Discard(DiscardReason::Invalid(
                    format!("endorsement targets a block with wrong slot. Block's parent: {}, endorsement: {}",
                            parent_in_own_thread.slot, endorsement.content.slot),
                )));
            }

            // note that the following aspects are checked in protocol
            // * PoS draws
            // * signature
            // * intra block endorsement reuse
            // * intra block index reuse
            // * slot in the same thread as block's slot
            // * slot is before the block's slot
            // * the endorsed block is the parent in the same thread
        }

        Ok(EndorsementsCheckOutcome::Proceed)
    }

    /// get genesis block ids
    pub fn get_genesis_block_ids(&self) -> &Vec<BlockId> {
        &self.genesis_hashes
    }

    /// Computes max cliques of compatible blocks
    pub fn compute_max_cliques(&self) -> Vec<PreHashSet<BlockId>> {
        let mut max_cliques: Vec<PreHashSet<BlockId>> = Vec::new();

        // algorithm adapted from IK_GPX as summarized in:
        //   Cazals et al., "A note on the problem of reporting maximal cliques"
        //   Theoretical Computer Science, 2008
        //   https://doi.org/10.1016/j.tcs.2008.05.010

        // stack: r, p, x
        let mut stack: Vec<(
            PreHashSet<BlockId>,
            PreHashSet<BlockId>,
            PreHashSet<BlockId>,
        )> = vec![(
            PreHashSet::<BlockId>::default(),
            self.gi_head.keys().cloned().collect(),
            PreHashSet::<BlockId>::default(),
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
            let u_set: PreHashSet<BlockId> =
                &p & &(&self.gi_head[&u_p] | &vec![u_p].into_iter().collect());
            for u_i in u_set.into_iter() {
                p.remove(&u_i);
                let u_i_set: PreHashSet<BlockId> = vec![u_i].into_iter().collect();
                let comp_n_u_i: PreHashSet<BlockId> = &self.gi_head[&u_i] | &u_i_set;
                stack.push((&r | &u_i_set, &p - &comp_n_u_i, &x - &comp_n_u_i));
                x.insert(u_i);
            }
        }
        if max_cliques.is_empty() {
            // make sure at least one clique remains
            max_cliques = vec![PreHashSet::<BlockId>::default()];
        }
        max_cliques
    }

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
    ) -> Result<()> {
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
                    children: vec![Default::default(); self.cfg.thread_count as usize],
                    is_final: false,
                    slot: add_block_slot,
                    fitness,
                }),
                storage,
            },
        );
        self.active_index.insert(add_block_id);

        // add as child to parents
        for (parent_h, _parent_period) in parents_hash_period.iter() {
            if let Some(BlockStatus::Active {
                a_block: a_parent, ..
            }) = self.block_statuses.get_mut(parent_h)
            {
                a_parent.children[add_block_slot.thread as usize]
                    .insert(add_block_id, add_block_slot.period);
            } else {
                return Err(GraphError::ContainerInconsistency(format!(
                    "inconsistency inside block statuses adding child {} of block {}",
                    add_block_id, parent_h
                )));
            }
        }

        // add as descendant to ancestors. Note: descendants are never removed.
        {
            let mut ancestors: VecDeque<BlockId> =
                parents_hash_period.iter().map(|(h, _)| *h).collect();
            let mut visited = PreHashSet::<BlockId>::default();
            while let Some(ancestor_h) = ancestors.pop_back() {
                if !visited.insert(ancestor_h) {
                    continue;
                }
                if let Some(BlockStatus::Active { a_block: ab, .. }) =
                    self.block_statuses.get_mut(&ancestor_h)
                {
                    ab.descendants.insert(add_block_id);
                    for (ancestor_parent_h, _) in ab.parents.iter() {
                        ancestors.push_front(*ancestor_parent_h);
                    }
                }
            }
        }

        // add incompatibilities to gi_head
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.add_incompatibilities",
            {}
        );
        for incomp_h in incomp.iter() {
            self.gi_head
                .get_mut(incomp_h)
                .ok_or_else(|| {
                    GraphError::MissingBlock(format!(
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
            self.max_cliques = self
                .compute_max_cliques()
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
        {
            let mut blockclique_i = 0usize;
            let mut max_clique_fitness = (0u64, num::BigInt::default());
            for (clique_i, clique) in self.max_cliques.iter_mut().enumerate() {
                clique.fitness = 0;
                clique.is_blockclique = false;
                let mut sum_hash = num::BigInt::default();
                for block_h in clique.block_ids.iter() {
                    clique.fitness = clique.fitness
                        .checked_add(
                            BlockGraph::get_full_active_block(&self.block_statuses, *block_h)
                                .ok_or_else(|| GraphError::ContainerInconsistency(format!("inconsistency inside block statuses computing fitness while adding {} - missing {}", add_block_id, block_h)))?
                                .0.fitness,
                        )
                        .ok_or(GraphError::FitnessOverflow)?;
                    sum_hash -=
                        num::BigInt::from_bytes_be(num::bigint::Sign::Plus, block_h.to_bytes());
                }
                let cur_fit = (clique.fitness, sum_hash);
                if cur_fit > max_clique_fitness {
                    blockclique_i = clique_i;
                    max_clique_fitness = cur_fit;
                }
            }
            self.max_cliques[blockclique_i].is_blockclique = true;
        }

        // update best parents
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.update_best_parents",
            {}
        );
        {
            // find blockclique
            let blockclique_i = self
                .max_cliques
                .iter()
                .position(|c| c.is_blockclique)
                .unwrap_or_default();
            let blockclique = &self.max_cliques[blockclique_i];

            // init best parents as latest_final_blocks_periods
            self.best_parents = self.latest_final_blocks_periods.clone();
            // for each blockclique block, set it as best_parent in its own thread
            // if its period is higher than the current best_parent in that thread
            for block_h in blockclique.block_ids.iter() {
                let b_slot = BlockGraph::get_full_active_block(&self.block_statuses, *block_h)
                    .ok_or_else(|| GraphError::ContainerInconsistency(format!("inconsistency inside block statuses updating best parents while adding {} - missing {}", add_block_id, block_h)))?
                    .0.slot;
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
        let stale_blocks = {
            let blockclique_i = self
                .max_cliques
                .iter()
                .position(|c| c.is_blockclique)
                .unwrap_or_default();
            let fitness_threshold = self.max_cliques[blockclique_i]
                .fitness
                .saturating_sub(self.cfg.delta_f0);
            // iterate from largest to smallest to minimize reallocations
            let mut indices: Vec<usize> = (0..self.max_cliques.len()).collect();
            indices
                .sort_unstable_by_key(|&i| std::cmp::Reverse(self.max_cliques[i].block_ids.len()));
            let mut high_set = PreHashSet::<BlockId>::default();
            let mut low_set = PreHashSet::<BlockId>::default();
            for clique_i in indices.into_iter() {
                if self.max_cliques[clique_i].fitness >= fitness_threshold {
                    high_set.extend(&self.max_cliques[clique_i].block_ids);
                } else {
                    low_set.extend(&self.max_cliques[clique_i].block_ids);
                }
            }
            self.max_cliques.retain(|c| c.fitness >= fitness_threshold);
            &low_set - &high_set
        };
        // mark stale blocks
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.mark_stale_blocks",
            {}
        );
        for stale_block_hash in stale_blocks.into_iter() {
            if let Some(BlockStatus::Active {
                a_block: active_block,
                storage: _storage,
            }) = self.block_statuses.remove(&stale_block_hash)
            {
                self.active_index.remove(&stale_block_hash);
                if active_block.is_final {
                    return Err(GraphError::ContainerInconsistency(format!("inconsistency inside block statuses removing stale blocks adding {} - block {} was already final", add_block_id, stale_block_hash)));
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
                let stale_block_fitness = active_block.fitness;
                self.max_cliques.iter_mut().for_each(|c| {
                    if c.block_ids.remove(&stale_block_hash) {
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
                    }) = self.block_statuses.get_mut(parent_h)
                    {
                        parent_active_block.children[active_block.slot.thread as usize]
                            .remove(&stale_block_hash);
                    }
                }

                massa_trace!("consensus.block_graph.add_block_to_graph.stale", {
                    "hash": stale_block_hash
                });

                // mark as stale
                self.new_stale_blocks.insert(
                    stale_block_hash,
                    (active_block.creator_address, active_block.slot),
                );
                self.block_statuses.insert(
                    stale_block_hash,
                    BlockStatus::Discarded {
                        slot: active_block.slot,
                        creator: active_block.creator_address,
                        parents: active_block.parents.iter().map(|(h, _)| *h).collect(),
                        reason: DiscardReason::Stale,
                        sequence_number: BlockGraph::new_sequence_number(
                            &mut self.sequence_counter,
                        ),
                    },
                );
                self.discarded_index.insert(stale_block_hash);
            } else {
                return Err(GraphError::ContainerInconsistency(format!("inconsistency inside block statuses removing stale blocks adding {} - block {} is missing", add_block_id, stale_block_hash)));
            }
        }

        // list final blocks
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.list_final_blocks",
            {}
        );
        let final_blocks = {
            // short-circuiting intersection of cliques from smallest to largest
            let mut indices: Vec<usize> = (0..self.max_cliques.len()).collect();
            indices.sort_unstable_by_key(|&i| self.max_cliques[i].block_ids.len());
            let mut final_candidates = self.max_cliques[indices[0]].block_ids.clone();
            for i in 1..indices.len() {
                final_candidates.retain(|v| self.max_cliques[i].block_ids.contains(v));
                if final_candidates.is_empty() {
                    break;
                }
            }

            // restrict search to cliques with high enough fitness, sort cliques by fitness (highest to lowest)
            massa_trace!(
                "consensus.block_graph.add_block_to_graph.list_final_blocks.restrict",
                {}
            );
            indices.retain(|&i| self.max_cliques[i].fitness > self.cfg.delta_f0);
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
                    let desc_fit: u64 =
                        BlockGraph::get_full_active_block(&self.block_statuses, candidate_h)
                            .ok_or_else(|| {
                                GraphError::MissingBlock(format!(
                                    "missing block when computing total fitness of descendants: {}",
                                    candidate_h
                                ))
                            })?
                            .0
                            .descendants
                            .intersection(&clique.block_ids)
                            .map(|h| {
                                if let Some(BlockStatus::Active { a_block: ab, .. }) =
                                    self.block_statuses.get(h)
                                {
                                    return ab.fitness;
                                }
                                0
                            })
                            .sum();
                    if desc_fit > self.cfg.delta_f0 {
                        // candidate is final
                        final_candidates.remove(&candidate_h);
                        final_blocks.insert(candidate_h);
                    }
                }
            }
            final_blocks
        };

        // mark final blocks and update latest_final_blocks_periods
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.mark_final_blocks",
            {}
        );
        for final_block_hash in final_blocks.into_iter() {
            // remove from gi_head
            if let Some(other_incomps) = self.gi_head.remove(&final_block_hash) {
                for other_incomp in other_incomps.into_iter() {
                    if let Some(other_incomp_lst) = self.gi_head.get_mut(&other_incomp) {
                        other_incomp_lst.remove(&final_block_hash);
                    }
                }
            }

            // mark as final and update latest_final_blocks_periods
            if let Some(BlockStatus::Active {
                a_block: final_block,
                ..
            }) = self.block_statuses.get_mut(&final_block_hash)
            {
                massa_trace!("consensus.block_graph.add_block_to_graph.final", {
                    "hash": final_block_hash
                });
                final_block.is_final = true;
                // remove from cliques
                let final_block_fitness = final_block.fitness;
                self.max_cliques.iter_mut().for_each(|c| {
                    if c.block_ids.remove(&final_block_hash) {
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
                        (final_block_hash, final_block.slot.period);
                }
                // update new final blocks list
                self.new_final_blocks.insert(final_block_hash);
            } else {
                return Err(GraphError::ContainerInconsistency(format!("inconsistency inside block statuses updating final blocks adding {} - block {} is missing", add_block_id, final_block_hash)));
            }
        }

        massa_trace!("consensus.block_graph.add_block_to_graph.end", {});
        Ok(())
    }

    fn list_required_active_blocks(&self) -> Result<PreHashSet<BlockId>> {
        // list all active blocks
        let mut retain_active: PreHashSet<BlockId> =
            PreHashSet::<BlockId>::with_capacity(self.active_index.len());

        let latest_final_blocks: Vec<BlockId> = self
            .latest_final_blocks_periods
            .iter()
            .map(|(hash, _)| *hash)
            .collect();

        // retain all non-final active blocks,
        // the current "best parents",
        // and the dependencies for both.
        for block_id in self.active_index.iter() {
            if let Some(BlockStatus::Active {
                a_block: active_block,
                ..
            }) = self.block_statuses.get(block_id)
            {
                if !active_block.is_final
                    || self.best_parents.iter().any(|(b, _p)| b == block_id)
                    || latest_final_blocks.contains(block_id)
                {
                    retain_active.extend(active_block.parents.iter().map(|(p, _)| *p));
                    retain_active.insert(*block_id);
                }
            }
        }

        // retain best parents
        retain_active.extend(self.best_parents.iter().map(|(b, _p)| *b));

        // retain last final blocks
        retain_active.extend(self.latest_final_blocks_periods.iter().map(|(h, _)| *h));

        for (thread, id) in latest_final_blocks.iter().enumerate() {
            let mut current_block_id = *id;
            while let Some((current_block, _)) = self.get_active_block(&current_block_id) {
                let parent_id = {
                    if !current_block.parents.is_empty() {
                        Some(current_block.parents[thread as usize].0)
                    } else {
                        None
                    }
                };

                // retain block
                retain_active.insert(current_block_id);

                // stop traversing when reaching a block with period number low enough
                // so that any of its operations will have their validity period expired at the latest final block in thread
                // note: one more is kept because of the way we iterate
                if current_block.slot.period
                    < self.latest_final_blocks_periods[thread]
                        .1
                        .saturating_sub(self.cfg.operation_validity_periods)
                {
                    break;
                }

                // if not genesis, traverse parent
                match parent_id {
                    Some(p_id) => current_block_id = p_id,
                    None => break,
                }
            }
        }

        // grow with parents & fill thread holes twice
        for _ in 0..2 {
            // retain the parents of the selected blocks
            let retain_clone = retain_active.clone();

            for retain_h in retain_clone.into_iter() {
                retain_active.extend(
                    self.get_active_block(&retain_h)
                        .ok_or_else(|| GraphError::ContainerInconsistency(format!("inconsistency inside block statuses pruning and retaining the parents of the selected blocks - {} is missing", retain_h)))?
                        .0.parents
                        .iter()
                        .map(|(b_id, _p)| *b_id),
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
                    .get_active_block(retain_h)
                    .ok_or_else(|| GraphError::ContainerInconsistency(format!("inconsistency inside block statuses pruning and finding earliest kept slots in each thread - {} is missing", retain_h)))?
                    .0.slot;
                earliest_retained_periods[retain_slot.thread as usize] = std::cmp::min(
                    earliest_retained_periods[retain_slot.thread as usize],
                    retain_slot.period,
                );
            }

            // fill up from the latest final block back to the earliest for each thread
            for thread in 0..self.cfg.thread_count {
                let mut cursor = self.latest_final_blocks_periods[thread as usize].0; // hash of tha latest final in that thread
                while let Some((c_block, _)) = self.get_active_block(&cursor) {
                    if c_block.slot.period < earliest_retained_periods[thread as usize] {
                        break;
                    }
                    retain_active.insert(cursor);
                    if c_block.parents.is_empty() {
                        // genesis
                        break;
                    }
                    cursor = c_block.parents[thread as usize].0;
                }
            }
        }

        Ok(retain_active)
    }

    /// prune active blocks and return final blocks, return discarded final blocks
    fn prune_active(&mut self) -> Result<PreHashMap<BlockId, ActiveBlock>> {
        // list required active blocks
        let mut retain_active = self.list_required_active_blocks()?;

        // retain extra history according to the config
        // this is useful to avoid desync on temporary connection loss
        for a_block in self.active_index.iter() {
            if let Some(BlockStatus::Active {
                a_block: active_block,
                ..
            }) = self.block_statuses.get(a_block)
            {
                let (_b_id, latest_final_period) =
                    self.latest_final_blocks_periods[active_block.slot.thread as usize];
                if active_block.slot.period
                    >= latest_final_period.saturating_sub(self.cfg.force_keep_final_periods)
                {
                    retain_active.insert(*a_block);
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
                    GraphError::MissingBlock(format!(
                        "missing block when removing unused final active blocks: {}",
                        discard_active_h
                    ))
                })?;
                block_slot = block.content.header.content.slot;
                block_creator = block.creator_address;
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
                return Err(GraphError::ContainerInconsistency(format!("inconsistency inside block statuses pruning and removing unused final active blocks - {} is missing", discard_active_h)));
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
                    sequence_number: BlockGraph::new_sequence_number(&mut self.sequence_counter),
                },
            );
            self.discarded_index.insert(discard_active_h);

            discarded_finals.insert(discard_active_h, *discarded_active);
        }

        Ok(discarded_finals)
    }

    fn promote_dep_tree(&mut self, hash: BlockId) -> Result<()> {
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
                *sequence_number = BlockGraph::new_sequence_number(&mut self.sequence_counter);
            }
        }
        Ok(())
    }

    fn prune_waiting_for_dependencies(&mut self) -> Result<()> {
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
                            GraphError::MissingBlock(format!(
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
                        self.new_stale_blocks
                            .insert(block_id, (header.creator_address, header.content.slot));
                    }
                    // transition to Discarded only if there is a reason
                    self.block_statuses.insert(
                        block_id,
                        BlockStatus::Discarded {
                            slot: header.content.slot,
                            creator: header.creator_address,
                            parents: header.content.parents.clone(),
                            reason,
                            sequence_number: BlockGraph::new_sequence_number(
                                &mut self.sequence_counter,
                            ),
                        },
                    );
                    self.discarded_index.insert(block_id);
                }
            }
        }

        Ok(())
    }

    fn prune_slot_waiting(&mut self) {
        if self.waiting_for_slot_index.len() <= self.cfg.max_future_processing_blocks {
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
        (self.cfg.max_future_processing_blocks..len_slot_waiting).for_each(|idx| {
            let (_slot, block_id) = &slot_waiting[idx];
            self.block_statuses.remove(block_id);
            self.waiting_for_slot_index.remove(block_id);
        });
    }

    fn prune_discarded(&mut self) -> Result<()> {
        if self.discarded_index.len() <= self.cfg.max_discarded_blocks {
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
        discard_hashes.truncate(self.discarded_index.len() - self.cfg.max_discarded_blocks);
        for (_, block_id) in discard_hashes.iter() {
            self.block_statuses.remove(block_id);
            self.discarded_index.remove(block_id);
        }
        Ok(())
    }

    /// prune and return final blocks, return discarded final blocks
    pub fn prune(&mut self) -> Result<PreHashMap<BlockId, ActiveBlock>> {
        let before = self.max_cliques.len();
        // Step 1: discard final blocks that are not useful to the graph anymore and return them
        let discarded_finals = self.prune_active()?;

        // Step 2: prune slot waiting blocks
        self.prune_slot_waiting();

        // Step 3: prune dependency waiting blocks
        self.prune_waiting_for_dependencies()?;

        // Step 4: prune discarded
        self.prune_discarded()?;

        let after = self.max_cliques.len();
        if before != after {
            debug!(
                "clique number went from {} to {} after pruning",
                before, after
            );
        }

        Ok(discarded_finals)
    }

    /// get the current block wish list, including the operations hash.
    pub fn get_block_wishlist(&self) -> Result<PreHashSet<BlockId>> {
        let mut wishlist = PreHashSet::<BlockId>::default();
        for block_id in self.waiting_for_dependencies_index.iter() {
            if let Some(BlockStatus::WaitingForDependencies {
                unsatisfied_dependencies,
                ..
            }) = self.block_statuses.get(block_id)
            {
                for unsatisfied_h in unsatisfied_dependencies.iter() {
                    if let Some(BlockStatus::WaitingForDependencies {
                        header_or_block: HeaderOrBlock::Block { .. },
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

    /// get clique count
    pub fn get_clique_count(&self) -> usize {
        self.max_cliques.len()
    }

    /// get the clique of higher fitness
    pub fn get_blockclique(&self) -> PreHashSet<BlockId> {
        self.max_cliques
            .iter()
            .enumerate()
            .find(|(_, c)| c.is_blockclique)
            .map_or_else(PreHashSet::<BlockId>::default, |(_, v)| v.block_ids.clone())
    }

    /// get the blockclique (or final) block ID at a given slot, if any
    pub fn get_blockclique_block_at_slot(&self, slot: &Slot) -> Option<BlockId> {
        // List all blocks at this slot.
        // The list should be small: make a copy of it to avoid holding the storage lock.
        let blocks_at_slot = {
            let storage_read = self.storage.read_blocks();
            match storage_read.get_blocks_by_slot(slot) {
                Some(v) => v.clone(),
                None => return None,
            };
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

    /// Clones all stored final blocks, not only the still-useful ones
    /// This is used when initializing Execution from Consensus.
    /// Since the Execution bootstrap snapshot is older than the Consensus snapshot,
    /// we might need to signal older final blocks for Execution to catch up.
    pub fn get_all_final_blocks(&self) -> HashMap<Slot, (BlockId, Storage)> {
        self.active_index
            .iter()
            .filter_map(|b_id| match self.get_active_block(b_id) {
                Some((a_b, storage)) if a_b.is_final => Some((a_b.slot, (*b_id, storage.clone()))),
                _ => None,
            })
            .collect()
    }

    /// Get the block id's to be propagated.
    /// Must be called by the consensus worker within `block_db_changed`.
    pub fn get_blocks_to_propagate(&mut self) -> PreHashMap<BlockId, Storage> {
        mem::take(&mut self.to_propagate)
    }

    /// Get the hashes of objects that were attack attempts.
    /// Must be called by the consensus worker within `block_db_changed`.
    pub fn get_attack_attempts(&mut self) -> Vec<BlockId> {
        mem::take(&mut self.attack_attempts)
    }

    /// Get the ids of blocks that became final.
    /// Must be called by the consensus worker within `block_db_changed`.
    pub fn get_new_final_blocks(&mut self) -> PreHashSet<BlockId> {
        mem::take(&mut self.new_final_blocks)
    }

    /// Get the ids of blocks that became stale.
    /// Must be called by the consensus worker within `block_db_changed`.
    pub fn get_new_stale_blocks(&mut self) -> PreHashMap<BlockId, (Address, Slot)> {
        mem::take(&mut self.new_stale_blocks)
    }
}
