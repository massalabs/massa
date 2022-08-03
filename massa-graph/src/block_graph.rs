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
    active_block::ActiveBlock, api::EndorsementInfo, clique::Clique, wrapped::WrappedContent,
    WrappedBlock, WrappedEndorsement,
};
use massa_models::{
    prehash::{BuildMap, Map, Set},
    WrappedHeader,
};
use massa_models::{
    Address, Block, BlockHeader, BlockHeaderSerializer, BlockId, BlockSerializer, EndorsementId,
    OperationId, OperationSearchResult, OperationSearchResultBlockStatus,
    OperationSearchResultStatus, Slot,
};
use massa_pos_exports::SelectorController;
use massa_signature::PublicKey;
use massa_storage::Storage;
use serde::{Deserialize, Serialize};
use std::collections::{hash_map, BTreeSet, HashMap, VecDeque};
use std::mem;
use std::{collections::HashSet, usize};
use tracing::{debug, info};

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum HeaderOrBlock {
    Header(WrappedHeader),
    Block(
        BlockId,
        Slot,
        Map<OperationId, (usize, u64)>,
        Map<EndorsementId, u32>,
    ),
}

impl HeaderOrBlock {
    /// Gets slot for that header or block
    pub fn get_slot(&self) -> Slot {
        match self {
            HeaderOrBlock::Header(header) => header.content.slot,
            HeaderOrBlock::Block(_, slot, ..) => *slot,
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
        unsatisfied_dependencies: Set<BlockId>,
        /// Used to limit and sort the number of blocks/headers waiting for dependencies
        sequence_number: u64,
    },
    /// The block was checked and included in the blockgraph
    Active(Box<ActiveBlock>),
    /// The block was discarded and is kept to avoid reprocessing it
    Discarded {
        /// Just the header of that block
        header: WrappedHeader,
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
    pub children: Vec<Set<BlockId>>,
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
            active_blocks: Map::with_capacity_and_hasher(
                block_graph.block_statuses.len(),
                BuildMap::default(),
            ),
            discarded_blocks: Map::with_capacity_and_hasher(
                block_graph.block_statuses.len(),
                BuildMap::default(),
            ),
            best_parents: block_graph.best_parents.clone(),
            latest_final_blocks_periods: block_graph.latest_final_blocks_periods.clone(),
            gi_head: block_graph.gi_head.clone(),
            max_cliques: block_graph.max_cliques.clone(),
        };

        let filter = |s| {
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
                BlockStatus::Discarded { header, reason, .. } => {
                    if filter(header.content.slot) {
                        export
                            .discarded_blocks
                            .insert(*hash, (reason.clone(), header.clone()));
                    }
                }
                BlockStatus::Active(a_block) => {
                    if filter(a_block.slot) {
                        let block = block_graph.storage.retrieve_block(hash).ok_or_else(|| {
                            GraphError::MissingBlock(format!(
                                "missing block in BlockGraphExport::extract_from: {}",
                                hash
                            ))
                        })?;
                        let stored_block = block.read();
                        export.active_blocks.insert(
                            *hash,
                            ExportCompiledBlock {
                                header: stored_block.content.header.clone(),
                                children: a_block
                                    .children
                                    .iter()
                                    .map(|thread| thread.keys().copied().collect::<Set<BlockId>>())
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
pub struct BlockGraphExport {
    /// Genesis blocks.
    pub genesis_blocks: Vec<BlockId>,
    /// Map of active blocks, were blocks are in their exported version.
    pub active_blocks: Map<BlockId, ExportCompiledBlock>,
    /// Finite cache of discarded blocks, in exported version.
    pub discarded_blocks: Map<BlockId, (DiscardReason, WrappedHeader)>,
    /// Best parents hashes in each thread.
    pub best_parents: Vec<(BlockId, u64)>,
    /// Latest final period and block hash in each thread.
    pub latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// Head of the incompatibility graph.
    pub gi_head: Map<BlockId, Set<BlockId>>,
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
    block_statuses: Map<BlockId, BlockStatus>,
    /// Ids of incoming blocks/headers
    incoming_index: Set<BlockId>,
    /// ids of waiting for slot blocks/headers
    waiting_for_slot_index: Set<BlockId>,
    /// ids of waiting for dependencies blocks/headers
    waiting_for_dependencies_index: Set<BlockId>,
    /// ids of active blocks
    active_index: Set<BlockId>,
    /// ids of discarded blocks
    discarded_index: Set<BlockId>,
    /// One (block id, period) per thread
    latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// One `(block id, period)` per thread TODO not sure I understand the difference with `latest_final_blocks_periods`
    best_parents: Vec<(BlockId, u64)>,
    /// Incompatibility graph: maps a block id to the block ids it is incompatible with
    /// One entry per Active Block
    pub gi_head: Map<BlockId, Set<BlockId>>,
    /// All the cliques
    max_cliques: Vec<Clique>,
    /// Blocks that need to be propagated
    to_propagate: Map<BlockId, (Set<OperationId>, Vec<EndorsementId>)>,
    /// List of block ids we think are attack attempts
    attack_attempts: Vec<BlockId>,
    /// Newly final blocks
    new_final_blocks: Set<BlockId>,
    /// Newly stale block mapped to creator and slot
    new_stale_blocks: Map<BlockId, (PublicKey, Slot)>,
    /// Shared storage,
    pub storage: Storage,
    /// Selector controller
    pub selector_controller: Box<dyn SelectorController>,
}

/// Possible output of a header check
#[derive(Debug)]
enum HeaderCheckOutcome {
    /// it's ok and here are some useful values
    Proceed {
        /// one (parent block id, parent's period) per thread
        parents_hash_period: Vec<(BlockId, u64)>,
        /// blocks that header depends on
        dependencies: Set<BlockId>,
        /// blocks that header is incompatible with
        incompatibilities: Set<BlockId>,
        /// number of incompatibilities that are inherited from the parents
        inherited_incompatibilities_count: usize,
    },
    /// there is something wrong with that header
    Discard(DiscardReason),
    /// it must wait for its slot to be fully processed
    WaitForSlot,
    /// it must wait for these block ids to be fully processed
    WaitForDependencies(Set<BlockId>),
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
/// * `serialization_context`: ref to a `SerializationContext` instance
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
                operations: Vec::new(),
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
        mut storage: Storage,
        selector_controller: Box<dyn SelectorController>,
    ) -> Result<Self> {
        // load genesis blocks

        let mut block_statuses = Map::default();
        let mut genesis_block_ids = Vec::with_capacity(cfg.thread_count as usize);
        for thread in 0u8..cfg.thread_count {
            let (block_id, block) = create_genesis_block(&cfg, thread).map_err(|err| {
                GraphError::GenesisCreationError(format!("genesis error {}", err))
            })?;
            genesis_block_ids.push(block_id);
            block_statuses.insert(
                block_id,
                BlockStatus::Active(Box::new(ActiveBlock {
                    creator_address: block.creator_address,
                    parents: Vec::new(),
                    children: vec![Map::default(); cfg.thread_count as usize],
                    dependencies: Set::<BlockId>::default(),
                    descendants: Set::<BlockId>::default(),
                    is_final: true,
                    operation_set: Default::default(),
                    endorsement_ids: Default::default(),
                    addresses_to_operations: Map::with_capacity_and_hasher(0, BuildMap::default()),
                    block_id,
                    addresses_to_endorsements: Default::default(),
                    slot: block.content.header.content.slot,
                })),
            );
            storage.store_block(block);
        }

        massa_trace!("consensus.block_graph.new", {});
        if let Some(boot_graph) = init {
            let mut res_graph = BlockGraph {
                cfg,
                sequence_counter: 0,
                genesis_hashes: genesis_block_ids,
                active_index: boot_graph.active_blocks.keys().copied().collect(),
                block_statuses: boot_graph
                    .active_blocks
                    .into_iter()
                    .map(|(b_id, exported_active_block)| {
                        // TODO: remove clone by doing a manual `into` below.
                        let block = exported_active_block.block.clone();
                        storage.store_block(block);
                        Ok((
                            b_id,
                            BlockStatus::Active(Box::new(exported_active_block.try_into()?)),
                        ))
                    })
                    .collect::<Result<_>>()?,
                incoming_index: Default::default(),
                waiting_for_slot_index: Default::default(),
                waiting_for_dependencies_index: Default::default(),
                discarded_index: Default::default(),
                latest_final_blocks_periods: boot_graph.latest_final_blocks_periods,
                best_parents: boot_graph.best_parents,
                gi_head: boot_graph.gi_head,
                max_cliques: boot_graph.max_cliques,
                to_propagate: Default::default(),
                attack_attempts: Default::default(),
                new_final_blocks: Default::default(),
                new_stale_blocks: Default::default(),
                storage,
                selector_controller,
            };
            // compute block descendants
            let active_blocks_map: Map<BlockId, Vec<BlockId>> = res_graph
                .block_statuses
                .iter()
                .filter_map(|(h, s)| {
                    if let BlockStatus::Active(a) = s {
                        Some((*h, a.parents.iter().map(|(ph, _)| *ph).collect()))
                    } else {
                        None
                    }
                })
                .collect();
            for (b_hash, b_parents) in active_blocks_map.into_iter() {
                let mut ancestors: VecDeque<BlockId> = b_parents.into_iter().collect();
                let mut visited: Set<BlockId> = Default::default();
                while let Some(ancestor_h) = ancestors.pop_back() {
                    if !visited.insert(ancestor_h) {
                        continue;
                    }
                    if let Some(BlockStatus::Active(ab)) =
                        res_graph.block_statuses.get_mut(&ancestor_h)
                    {
                        ab.descendants.insert(b_hash);
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
                gi_head: Map::default(),
                max_cliques: vec![Clique {
                    block_ids: Set::<BlockId>::default(),
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
        let mut required_final_blocks: Set<_> = self.list_required_active_blocks()?;
        required_final_blocks.retain(|b_id| {
            if let Some(BlockStatus::Active(a_block)) = self.block_statuses.get(b_id) {
                if a_block.is_final {
                    // filter only final actives
                    return true;
                }
            }
            false
        });
        let mut active_blocks: Map<BlockId, ExportActiveBlock> =
            Map::with_capacity_and_hasher(required_final_blocks.len(), BuildMap::default());
        for b_id in &required_final_blocks {
            if let Some(BlockStatus::Active(a_block)) = self.block_statuses.get(b_id) {
                let block = self.storage.retrieve_block(b_id).ok_or_else(|| {
                    GraphError::MissingBlock(format!(
                        "missing block in export_bootstrap_graph: {}",
                        b_id
                    ))
                })?;
                let stored_block = block.read().clone();
                active_blocks.insert(
                    *b_id,
                    ExportActiveBlock {
                        block: stored_block,
                        block_id: *b_id,
                        parents: a_block.parents.clone(),
                        children: a_block
                            .children
                            .iter()
                            .map(|thread_children| {
                                thread_children
                                    .iter()
                                    .filter_map(|(child_id, child_period)| {
                                        if !required_final_blocks.contains(child_id) {
                                            return None;
                                        }
                                        Some((*child_id, *child_period))
                                    })
                                    .collect()
                            })
                            .collect(),
                        dependencies: a_block.dependencies.clone(),
                        is_final: a_block.is_final,
                    },
                );
            } else {
                return Err(GraphError::ContainerInconsistency(format!(
                    "block {} was expected to be active but wasn't on bootstrap graph export",
                    b_id
                )));
            }
        }

        Ok(BootstrapableGraph {
            active_blocks,
            best_parents: self.latest_final_blocks_periods.clone(),
            latest_final_blocks_periods: self.latest_final_blocks_periods.clone(),
            gi_head: self.gi_head.clone(),
            max_cliques: self.max_cliques.clone(),
        })
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
    pub fn get_block_ids_by_creator(&self, address: &Address) -> Map<BlockId, Status> {
        // iterate on active (final and non-final) blocks
        self.active_index
            .iter()
            .filter_map(|block_id| match self.block_statuses.get(block_id) {
                Some(BlockStatus::Active(active_block)) => {
                    if active_block.creator_address == *address {
                        Some((
                            *block_id,
                            if active_block.is_final {
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

    /// get operation info by involved address
    pub fn get_operations_involving_address(
        &self,
        address: &Address,
    ) -> Result<Map<OperationId, OperationSearchResult>> {
        let mut res: Map<OperationId, OperationSearchResult> = Default::default();
        'outer: for b_id in self.active_index.iter() {
            if let Some(BlockStatus::Active(active_block)) = self.block_statuses.get(b_id) {
                if let Some(ops) = active_block.addresses_to_operations.get(address) {
                    let stored_block = self.storage.retrieve_block(b_id).ok_or_else(|| {
                        GraphError::MissingBlock(format!(
                            "missing block in get_operations_involving_address: {}",
                            b_id
                        ))
                    })?;
                    let stored_block = stored_block.read();
                    for op in ops.iter() {
                        let (idx, _) = active_block.operation_set.get(op).ok_or_else(|| {
                            GraphError::ContainerInconsistency(format!("op {} should be here", op))
                        })?;
                        let search = OperationSearchResult {
                            op: stored_block.content.operations[*idx].clone(),
                            in_pool: false,
                            in_blocks: vec![(*b_id, (*idx, active_block.is_final))]
                                .into_iter()
                                .collect(),
                            status: OperationSearchResultStatus::InBlock(
                                OperationSearchResultBlockStatus::Active,
                            ),
                        };
                        if let Some(old_search) = res.get_mut(op) {
                            old_search.extend(&search);
                        } else {
                            res.insert(*op, search);
                        }
                        if res.len() >= self.cfg.max_item_return_count {
                            break 'outer;
                        }
                    }
                }
            }
        }
        Ok(res)
    }

    /// Gets whole compiled block corresponding to given hash, if it is active.
    ///
    /// # Argument
    /// * `block_id`: block ID
    pub fn get_active_block(&self, block_id: &BlockId) -> Option<&ActiveBlock> {
        BlockGraph::get_full_active_block(&self.block_statuses, *block_id)
    }

    /// get export version of a block
    pub fn get_export_block_status(&self, block_id: &BlockId) -> Result<Option<ExportBlockStatus>> {
        let block_status = match self.block_statuses.get(block_id) {
            None => return Ok(None),
            Some(block_status) => block_status,
        };
        let export = match block_status {
            BlockStatus::Incoming(_) => ExportBlockStatus::Incoming,
            BlockStatus::WaitingForSlot(_) => ExportBlockStatus::WaitingForSlot,
            BlockStatus::WaitingForDependencies { .. } => ExportBlockStatus::WaitingForDependencies,
            BlockStatus::Active(active_block) => {
                let block = self.storage.retrieve_block(block_id).ok_or_else(|| {
                    GraphError::MissingBlock(format!(
                        "missing block in get_export_block_status: {}",
                        block_id
                    ))
                })?;
                let stored_block = block.read();
                if active_block.is_final {
                    ExportBlockStatus::Final(stored_block.content.clone())
                } else {
                    ExportBlockStatus::Active(stored_block.content.clone())
                }
            }
            BlockStatus::Discarded { reason, .. } => ExportBlockStatus::Discarded(reason.clone()),
        };
        Ok(Some(export))
    }

    /// Retrieves operations from operation Ids
    pub fn get_operations(
        &self,
        operation_ids: Set<OperationId>,
    ) -> Result<Map<OperationId, OperationSearchResult>> {
        // The search result.
        let mut res: Map<OperationId, OperationSearchResult> = Default::default();

        // For each operation id we are searching for.
        for op_id in operation_ids.into_iter() {
            // The operation corresponding to the id, initially none.
            let mut operation = None;

            // The active blocks in which the operation is found.
            let mut in_blocks: Map<BlockId, (usize, bool)> = Default::default();

            for block_id in self.active_index.iter() {
                if let Some(BlockStatus::Active(active_block)) = self.block_statuses.get(block_id) {
                    // If the operation is found in the active block.
                    if let Some((idx, _)) = active_block.operation_set.get(&op_id) {
                        // If this is the first time we encounter the operation as present in an active block.
                        if operation.is_none() {
                            let stored_block =
                                self.storage.retrieve_block(block_id).ok_or_else(|| {
                                    GraphError::MissingBlock(format!(
                                        "missing block in get_operations: {}",
                                        block_id
                                    ))
                                })?;
                            let stored_block = stored_block.read();

                            // Clone the operation.
                            operation = Some(stored_block.content.operations[*idx].clone());
                        }
                        in_blocks.insert(*block_id, (*idx, active_block.is_final));
                    }
                }
            }

            // If we found the operation in at least one active block.
            if let Some(op) = operation {
                let result = OperationSearchResult {
                    op,
                    in_pool: false,
                    in_blocks,
                    status: OperationSearchResultStatus::InBlock(
                        OperationSearchResultBlockStatus::Active,
                    ),
                };
                res.insert(op_id, result);
            }
        }
        Ok(res)
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
        operation_set: Map<OperationId, (usize, u64)>,
        endorsement_ids: Map<EndorsementId, u32>,
        current_slot: Option<Slot>,
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
                vac.insert(BlockStatus::Incoming(HeaderOrBlock::Block(
                    block_id,
                    slot,
                    operation_set,
                    endorsement_ids,
                )));
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
                    *header_or_block =
                        HeaderOrBlock::Block(block_id, slot, operation_set, endorsement_ids);
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
                    *header_or_block =
                        HeaderOrBlock::Block(block_id, slot, operation_set, endorsement_ids);
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
            valid_block_addresses_to_operations,
            valid_block_addresses_to_endorsements,
            valid_block_creator,
            valid_block_slot,
            valid_block_parents_hash_period,
            valid_block_deps,
            valid_block_incomp,
            valid_block_inherited_incomp_count,
            valid_block_operation_set,
            valid_block_endorsement_ids,
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
            Some(BlockStatus::Active(_)) => {
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
                        let mut dependencies = Set::<BlockId>::default();
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
                                .insert(block_id, (header.creator_public_key, header.content.slot));
                        }
                        // discard
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::Discarded {
                                header,
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
            Some(BlockStatus::Incoming(HeaderOrBlock::Block(..))) => {
                massa_trace!("consensus.block_graph.process.incoming_block", {
                    "block_id": block_id
                });
                let block = self.storage.retrieve_block(&block_id).ok_or_else(|| {
                    GraphError::MissingBlock(format!(
                        "missing block in processing incoming block: {}",
                        block_id
                    ))
                })?;
                let stored_block = block.read();
                let (_block_id, slot, operation_set, endorsement_ids) =
                    if let Some(BlockStatus::Incoming(HeaderOrBlock::Block(
                        block_id,
                        slot,
                        operation_set,
                        endorsement_ids,
                    ))) = self.block_statuses.remove(&block_id)
                    {
                        self.incoming_index.remove(&block_id);
                        (block_id, slot, operation_set, endorsement_ids)
                    } else {
                        return Err(GraphError::ContainerInconsistency(format!(
                            "inconsistency inside block statuses removing incoming block {}",
                            block_id
                        )));
                    };
                match self.check_header(&block_id, &stored_block.content.header, current_slot)? {
                    HeaderCheckOutcome::Proceed {
                        parents_hash_period,
                        dependencies,
                        incompatibilities,
                        inherited_incompatibilities_count,
                    } => {
                        // block is valid: remove it from Incoming and return it
                        massa_trace!("consensus.block_graph.process.incoming_block.valid", {
                            "block_id": block_id
                        });
                        (
                            stored_block.involved_addresses(&operation_set)?,
                            stored_block.addresses_to_endorsements()?,
                            stored_block.content.header.creator_public_key,
                            slot,
                            parents_hash_period,
                            dependencies,
                            incompatibilities,
                            inherited_incompatibilities_count,
                            operation_set,
                            endorsement_ids,
                        )
                    }
                    HeaderCheckOutcome::WaitForDependencies(dependencies) => {
                        // set as waiting dependencies
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::WaitingForDependencies {
                                header_or_block: HeaderOrBlock::Block(
                                    block_id,
                                    slot,
                                    operation_set,
                                    endorsement_ids,
                                ),
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
                            BlockStatus::WaitingForSlot(HeaderOrBlock::Block(
                                block_id,
                                slot,
                                operation_set,
                                endorsement_ids,
                            )),
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
                                    stored_block.content.header.creator_public_key,
                                    stored_block.content.header.content.slot,
                                ),
                            );
                        }
                        // add to discard
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::Discarded {
                                header: stored_block.content.header.clone(),
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
            valid_block_deps,
            valid_block_incomp,
            valid_block_inherited_incomp_count,
            valid_block_operation_set,
            valid_block_endorsement_ids,
            valid_block_addresses_to_operations,
            valid_block_addresses_to_endorsements,
        )?;

        // if the block was added, update linked dependencies and mark satisfied ones for recheck
        if let Some(BlockStatus::Active(active)) = self.block_statuses.get(&block_id) {
            massa_trace!("consensus.block_graph.process.is_active", {
                "block_id": block_id
            });
            self.to_propagate.insert(
                block_id,
                (
                    active.operation_set.keys().copied().collect(),
                    active.endorsement_ids.keys().copied().collect(),
                ),
            );
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
        block_statuses: &Map<BlockId, BlockStatus>,
        block_id: BlockId,
    ) -> Option<&ActiveBlock> {
        match block_statuses.get(&block_id) {
            Some(BlockStatus::Active(active_block)) => Some(active_block),
            _ => None,
        }
    }

    /// Gets a block and all its descendants
    ///
    /// # Argument
    /// * hash : hash of the given block
    fn get_active_block_and_descendants(&self, block_id: &BlockId) -> Result<Set<BlockId>> {
        let mut to_visit = vec![*block_id];
        let mut result = Set::<BlockId>::default();
        while let Some(visit_h) = to_visit.pop() {
            if !result.insert(visit_h) {
                continue; // already visited
            }
            BlockGraph::get_full_active_block(&self.block_statuses, visit_h)
                .ok_or_else(|| GraphError::ContainerInconsistency(format!("inconsistency inside block statuses iterating through descendants of {} - missing {}", block_id, visit_h)))?
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
        let mut deps = Set::<BlockId>::default();
        let mut incomp = Set::<BlockId>::default();
        let mut missing_deps = Set::<BlockId>::default();
        let creator_addr = header.creator_address;

        // basic structural checks
        if header.content.parents.len() != (self.cfg.thread_count as usize)
            || header.content.slot.period == 0
            || header.content.slot.thread >= self.cfg.thread_count
        {
            return Ok(HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                "Basic structural header checks failed".to_string(),
            )));
        }

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
        // note: do this AFTER TooMuchInTheFuture checks
        //       to avoid doing too many draws to check blocks in the distant future
        let slot_draw_address = match self.selector_controller.get_producer(header.content.slot) {
            Ok(draw) => draw,
            Err(_) => return Ok(HeaderCheckOutcome::WaitForSlot),
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
        let parent_set: Set<BlockId> = header.content.parents.iter().copied().collect();
        deps.extend(&parent_set);
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
                Some(BlockStatus::Active(parent)) => {
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
                let parent = self.get_active_block(&parent_h).ok_or_else(|| {
                    GraphError::ContainerInconsistency(format!(
                        "inconsistency inside block statuses searching parent {} of block {}",
                        parent_h, block_id
                    ))
                })?;
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
                    deps.insert(gp_h);
                    match self.block_statuses.get(&gp_h) {
                        // this grandpa is discarded
                        Some(BlockStatus::Discarded { reason, .. }) => {
                            return Ok(HeaderCheckOutcome::Discard(reason.clone()));
                        }
                        // this grandpa is active
                        Some(BlockStatus::Active(gp)) => {
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
                        // this grandpa is missing or queued
                        _ => {
                            if self.genesis_hashes.contains(&gp_h) {
                                // forbid depending on discarded genesis block
                                return Ok(HeaderCheckOutcome::Discard(DiscardReason::Stale));
                            }
                            missing_deps.insert(gp_h);
                        }
                    }
                }
            }
        }
        if !missing_deps.is_empty() {
            return Ok(HeaderCheckOutcome::WaitForDependencies(missing_deps));
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
        })?;

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
                    .ok_or_else(|| GraphError::ContainerInconsistency(format!("inconsistency inside block statuses searching {} while checking grandpa incompatibility of block {}",cur_h,  block_id)))?;

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
                    let block = self
                        .storage
                        .retrieve_block(&cur_b.block_id)
                        .ok_or_else(|| {
                            GraphError::MissingBlock(format!(
                                "missing block in grandpa incomp test: {}",
                                cur_b.block_id
                            ))
                        })?;
                    let stored_block = block.read();
                    stored_block.content.header.content.parents[header.content.slot.thread as usize]
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
                    if let Some(BlockStatus::Active(a)) = self.block_statuses.get(h) {
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
            dependencies: deps,
            incompatibilities: incomp,
            inherited_incompatibilities_count: inherited_incomp_count,
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
            Ok(sel) => sel.endorsments,
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
    pub fn compute_max_cliques(&self) -> Vec<Set<BlockId>> {
        let mut max_cliques: Vec<Set<BlockId>> = Vec::new();

        // algorithm adapted from IK_GPX as summarized in:
        //   Cazals et al., "A note on the problem of reporting maximal cliques"
        //   Theoretical Computer Science, 2008
        //   https://doi.org/10.1016/j.tcs.2008.05.010

        // stack: r, p, x
        let mut stack: Vec<(Set<BlockId>, Set<BlockId>, Set<BlockId>)> = vec![(
            Set::<BlockId>::default(),
            self.gi_head.keys().cloned().collect(),
            Set::<BlockId>::default(),
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
            let u_set: Set<BlockId> =
                &p & &(&self.gi_head[&u_p] | &vec![u_p].into_iter().collect());
            for u_i in u_set.into_iter() {
                p.remove(&u_i);
                let u_i_set: Set<BlockId> = vec![u_i].into_iter().collect();
                let comp_n_u_i: Set<BlockId> = &self.gi_head[&u_i] | &u_i_set;
                stack.push((&r | &u_i_set, &p - &comp_n_u_i, &x - &comp_n_u_i));
                x.insert(u_i);
            }
        }
        if max_cliques.is_empty() {
            // make sure at least one clique remains
            max_cliques = vec![Set::<BlockId>::default()];
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
        deps: Set<BlockId>,
        incomp: Set<BlockId>,
        inherited_incomp_count: usize,
        operation_set: Map<OperationId, (usize, u64)>,
        endorsement_ids: Map<EndorsementId, u32>,
        addresses_to_operations: Map<Address, Set<OperationId>>,
        addresses_to_endorsements: Map<Address, Set<EndorsementId>>,
    ) -> Result<()> {
        massa_trace!("consensus.block_graph.add_block_to_graph", {
            "block_id": add_block_id
        });
        // add block to status structure
        self.block_statuses.insert(
            add_block_id,
            BlockStatus::Active(Box::new(ActiveBlock {
                creator_address: Address::from_public_key(&add_block_creator),
                parents: parents_hash_period.clone(),
                dependencies: deps,
                descendants: Set::<BlockId>::default(),
                block_id: add_block_id,
                children: vec![Default::default(); self.cfg.thread_count as usize],
                is_final: false,
                operation_set,
                endorsement_ids,
                addresses_to_operations,
                addresses_to_endorsements,
                slot: add_block_slot,
            })),
        );
        self.active_index.insert(add_block_id);

        // add as child to parents
        for (parent_h, _parent_period) in parents_hash_period.iter() {
            if let Some(BlockStatus::Active(a_parent)) = self.block_statuses.get_mut(parent_h) {
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
            let mut visited = Set::<BlockId>::default();
            while let Some(ancestor_h) = ancestors.pop_back() {
                if !visited.insert(ancestor_h) {
                    continue;
                }
                if let Some(BlockStatus::Active(ab)) = self.block_statuses.get_mut(&ancestor_h) {
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
                                .fitness(),
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
                    .slot;
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
            let mut high_set = Set::<BlockId>::default();
            let mut low_set = Set::<BlockId>::default();
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
            if let Some(BlockStatus::Active(active_block)) =
                self.block_statuses.remove(&stale_block_hash)
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
                let stale_block_fitness = active_block.fitness();
                self.max_cliques.iter_mut().for_each(|c| {
                    if c.block_ids.remove(&stale_block_hash) {
                        c.fitness -= stale_block_fitness;
                    }
                });
                self.max_cliques.retain(|c| !c.block_ids.is_empty()); // remove empty cliques
                if self.max_cliques.is_empty() {
                    // make sure at least one clique remains
                    self.max_cliques = vec![Clique {
                        block_ids: Set::<BlockId>::default(),
                        fitness: 0,
                        is_blockclique: true,
                    }];
                }

                // remove from parent's children
                for (parent_h, _parent_period) in active_block.parents.iter() {
                    if let Some(BlockStatus::Active(parent_active_block)) =
                        self.block_statuses.get_mut(parent_h)
                    {
                        parent_active_block.children[active_block.slot.thread as usize]
                            .remove(&stale_block_hash);
                    }
                }

                massa_trace!("consensus.block_graph.add_block_to_graph.stale", {
                    "hash": stale_block_hash
                });

                let (creator, header) = {
                    let block = self
                        .storage
                        .retrieve_block(&active_block.block_id)
                        .ok_or_else(|| {
                            GraphError::MissingBlock(format!(
                                "missing block when adding block to graph stale: {}",
                                active_block.block_id
                            ))
                        })?;
                    let stored_block = block.read();
                    (
                        stored_block.content.header.creator_public_key,
                        stored_block.content.header.clone(),
                    )
                };

                // mark as stale
                self.new_stale_blocks
                    .insert(stale_block_hash, (creator, active_block.slot));
                self.block_statuses.insert(
                    stale_block_hash,
                    BlockStatus::Discarded {
                        header,
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

            let mut final_blocks = Set::<BlockId>::default();
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
                            .descendants
                            .intersection(&clique.block_ids)
                            .map(|h| {
                                if let Some(BlockStatus::Active(ab)) = self.block_statuses.get(h) {
                                    return ab.fitness();
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
            if let Some(BlockStatus::Active(final_block)) =
                self.block_statuses.get_mut(&final_block_hash)
            {
                massa_trace!("consensus.block_graph.add_block_to_graph.final", {
                    "hash": final_block_hash
                });
                final_block.is_final = true;
                // remove from cliques
                let final_block_fitness = final_block.fitness();
                self.max_cliques.iter_mut().for_each(|c| {
                    if c.block_ids.remove(&final_block_hash) {
                        c.fitness -= final_block_fitness;
                    }
                });
                self.max_cliques.retain(|c| !c.block_ids.is_empty()); // remove empty cliques
                if self.max_cliques.is_empty() {
                    // make sure at least one clique remains
                    self.max_cliques = vec![Clique {
                        block_ids: Set::<BlockId>::default(),
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

    fn list_required_active_blocks(&self) -> Result<Set<BlockId>> {
        // list all active blocks
        let mut retain_active: Set<BlockId> =
            Set::<BlockId>::with_capacity_and_hasher(self.active_index.len(), BuildMap::default());

        let latest_final_blocks: Vec<BlockId> = self
            .latest_final_blocks_periods
            .iter()
            .map(|(hash, _)| *hash)
            .collect();

        // retain all non-final active blocks,
        // the current "best parents",
        // and the dependencies for both.
        for block_id in self.active_index.iter() {
            if let Some(BlockStatus::Active(active_block)) = self.block_statuses.get(block_id) {
                if !active_block.is_final
                    || self.best_parents.iter().any(|(b, _p)| b == block_id)
                    || latest_final_blocks.contains(block_id)
                {
                    retain_active.extend(active_block.dependencies.clone());
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
            while let Some(current_block) = self.get_active_block(&current_block_id) {
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
                        .parents
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
                    .slot;
                earliest_retained_periods[retain_slot.thread as usize] = std::cmp::min(
                    earliest_retained_periods[retain_slot.thread as usize],
                    retain_slot.period,
                );
            }

            // fill up from the latest final block back to the earliest for each thread
            for thread in 0..self.cfg.thread_count {
                let mut cursor = self.latest_final_blocks_periods[thread as usize].0; // hash of tha latest final in that thread
                while let Some(c_block) = self.get_active_block(&cursor) {
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
    fn prune_active(&mut self) -> Result<Map<BlockId, ActiveBlock>> {
        // list required active blocks
        let mut retain_active = self.list_required_active_blocks()?;

        // retain extra history according to the config
        // this is useful to avoid desync on temporary connection loss
        for a_block in self.active_index.iter() {
            if let Some(BlockStatus::Active(active_block)) = self.block_statuses.get(a_block) {
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
        let mut discarded_finals: Map<BlockId, ActiveBlock> = Map::default();
        let to_remove: Vec<BlockId> = self
            .active_index
            .difference(&retain_active)
            .copied()
            .collect();
        for discard_active_h in to_remove {
            let block = self
                .storage
                .retrieve_block(&discard_active_h)
                .ok_or_else(|| {
                    GraphError::MissingBlock(format!(
                        "missing block when removing unused final active blocks: {}",
                        discard_active_h
                    ))
                })?;
            let stored_block = block.read();

            let discarded_active = if let Some(BlockStatus::Active(discarded_active)) =
                self.block_statuses.remove(&discard_active_h)
            {
                self.active_index.remove(&discard_active_h);
                discarded_active
            } else {
                return Err(GraphError::ContainerInconsistency(format!("inconsistency inside block statuses pruning and removing unused final active blocks - {} is missing", discard_active_h)));
            };

            // remove from parent's children
            for (parent_h, _parent_period) in discarded_active.parents.iter() {
                if let Some(BlockStatus::Active(parent_active_block)) =
                    self.block_statuses.get_mut(parent_h)
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
                    header: stored_block.content.header.clone(),
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
        let mut to_promote: Map<BlockId, (Slot, u64)> = Map::default();
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
        let mut to_discard: Map<BlockId, Option<DiscardReason>> = Map::default();
        let mut to_keep: Map<BlockId, (u64, Slot)> = Map::default();

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
                    HeaderOrBlock::Block(block_id, ..) => {
                        let block = self.storage.retrieve_block(&block_id).ok_or_else(|| {
                            GraphError::MissingBlock(format!(
                                "missing block when pruning waiting for deps: {}",
                                block_id
                            ))
                        })?;
                        let stored_block = block.read();
                        stored_block.content.header.clone()
                    }
                };
                massa_trace!("consensus.block_graph.prune_waiting_for_dependencies", {"hash": block_id, "reason": reason_opt});

                // Prune shared storage
                // TODO manage properly
                self.storage.remove_blocks(&[block_id]);

                if let Some(reason) = reason_opt {
                    // add to stats if reason is Stale
                    if reason == DiscardReason::Stale {
                        self.new_stale_blocks
                            .insert(block_id, (header.creator_public_key, header.content.slot));
                    }
                    // transition to Discarded only if there is a reason
                    self.block_statuses.insert(
                        block_id,
                        BlockStatus::Discarded {
                            header,
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
        let mut to_prune: Vec<BlockId> =
            Vec::with_capacity(len_slot_waiting - self.cfg.max_future_processing_blocks);
        (self.cfg.max_future_processing_blocks..len_slot_waiting).for_each(|idx| {
            let (_slot, block_id) = &slot_waiting[idx];
            self.block_statuses.remove(block_id);
            self.waiting_for_slot_index.remove(block_id);
            to_prune.push(*block_id);
        });
        // Prune shared storage
        // TODO manage properly
        self.storage.remove_blocks(&to_prune);
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
        // Prune shared storage
        // TODO manage properly
        let ids: Vec<BlockId> = discard_hashes.into_iter().map(|(_, id)| id).collect();
        self.storage.remove_blocks(&ids);
        Ok(())
    }

    /// prune and return final blocks, return discarded final blocks
    pub fn prune(&mut self) -> Result<Map<BlockId, ActiveBlock>> {
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

    /// get the current block wish list
    pub fn get_block_wishlist(&self) -> Result<Set<BlockId>> {
        let mut wishlist = Set::<BlockId>::default();
        for block_id in self.waiting_for_dependencies_index.iter() {
            if let Some(BlockStatus::WaitingForDependencies {
                unsatisfied_dependencies,
                ..
            }) = self.block_statuses.get(block_id)
            {
                for unsatisfied_h in unsatisfied_dependencies.iter() {
                    if let Some(BlockStatus::WaitingForDependencies {
                        header_or_block: HeaderOrBlock::Block(..),
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
    pub fn get_blockclique(&self) -> Set<BlockId> {
        self.max_cliques
            .iter()
            .enumerate()
            .find(|(_, c)| c.is_blockclique)
            .map_or_else(Set::<BlockId>::default, |(_, v)| v.block_ids.clone())
    }

    /// Clones all stored final blocks, not only the still-useful ones
    /// This is used when initializing Execution from Consensus.
    /// Since the Execution bootstrap snapshot is older than the Consensus snapshot,
    /// we might need to signal older final blocks for Execution to catch up.
    pub fn get_all_final_blocks(&self) -> HashMap<Slot, BlockId> {
        self.active_index
            .iter()
            .filter_map(|b_id| {
                if let Some(a_b) = self.get_active_block(b_id) {
                    if a_b.is_final {
                        return Some((a_b.slot, *b_id));
                    }
                }
                None
            })
            .collect()
    }

    /// Get the block id's to be propagated.
    /// Must be called by the consensus worker within `block_db_changed`.
    pub fn get_blocks_to_propagate(
        &mut self,
    ) -> Map<BlockId, (Set<OperationId>, Vec<EndorsementId>)> {
        mem::take(&mut self.to_propagate)
    }

    /// Get the hashes of objects that were attack attempts.
    /// Must be called by the consensus worker within `block_db_changed`.
    pub fn get_attack_attempts(&mut self) -> Vec<BlockId> {
        mem::take(&mut self.attack_attempts)
    }

    /// Get the ids of blocks that became final.
    /// Must be called by the consensus worker within `block_db_changed`.
    pub fn get_new_final_blocks(&mut self) -> Set<BlockId> {
        mem::take(&mut self.new_final_blocks)
    }

    /// Get the ids of blocks that became stale.
    /// Must be called by the consensus worker within `block_db_changed`.
    pub fn get_new_stale_blocks(&mut self) -> Map<BlockId, (PublicKey, Slot)> {
        mem::take(&mut self.new_stale_blocks)
    }

    /// endorsement info by involved address
    pub fn get_endorsement_by_address(
        &self,
        address: Address,
    ) -> Result<Map<EndorsementId, WrappedEndorsement>> {
        let mut res: Map<EndorsementId, WrappedEndorsement> = Default::default();
        for b_id in self.active_index.iter() {
            if let Some(BlockStatus::Active(ab)) = self.block_statuses.get(b_id) {
                if let Some(eds) = ab.addresses_to_endorsements.get(&address) {
                    let block = self.storage.retrieve_block(b_id).ok_or_else(|| {
                        GraphError::MissingBlock(format!(
                            "missing block when getting endorsement by address: {}",
                            b_id
                        ))
                    })?;
                    let endorsements = block.read().content.header.content.endorsements.clone();
                    for e in endorsements {
                        let id = e.id;
                        if eds.contains(&id) {
                            res.insert(id, e);
                        }
                    }
                }
            }
        }
        Ok(res)
    }

    /// endorsement info by id
    pub fn get_endorsement_by_id(
        &self,
        endorsements: Set<EndorsementId>,
    ) -> Result<Map<EndorsementId, EndorsementInfo>> {
        // iterate on active (final and non-final) blocks

        let mut res = Map::default();
        for block_id in self.active_index.iter() {
            if let Some(BlockStatus::Active(ab)) = self.block_statuses.get(block_id) {
                let block = self.storage.retrieve_block(block_id).ok_or_else(|| {
                    GraphError::MissingBlock(format!(
                        "missing block when getting endorsement by id: {}",
                        block_id
                    ))
                })?;
                let stored_block = block.read();
                // list blocks with wanted endorsements
                if endorsements
                    .intersection(&ab.endorsement_ids.keys().copied().collect())
                    .collect::<HashSet<_>>()
                    .is_empty()
                {
                    for e in stored_block.content.header.content.endorsements.iter() {
                        let id = e.id;
                        if endorsements.contains(&id) {
                            res.entry(id)
                                .and_modify(|EndorsementInfo { in_blocks, .. }| {
                                    in_blocks.push(*block_id)
                                })
                                .or_insert(EndorsementInfo {
                                    id,
                                    in_pool: false,
                                    in_blocks: vec![*block_id],
                                    is_final: ab.is_final,
                                    endorsement: e.clone(),
                                });
                        }
                    }
                }
            }
        }
        Ok(res)
    }
}
