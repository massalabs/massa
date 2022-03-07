// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! All information concerning blocks, the block graph and cliques is managed here.
use crate::{
    bootstrapable_graph::BootstrapableGraph,
    error::{GraphError, GraphResult as Result},
    export_active_block::ExportActiveBlock,
    ledger::{read_genesis_ledger, Ledger, LedgerSubset, OperationLedgerInterface},
    settings::GraphConfig,
    LedgerConfig,
};
use massa_hash::hash::Hash;
use massa_logging::massa_trace;
use massa_models::{
    active_block::ActiveBlock,
    api::EndorsementInfo,
    rolls::{RollCounts, RollUpdate, RollUpdates},
    Operation, SignedHeader,
};
use massa_models::{clique::Clique, signed::Signable};
use massa_models::{ledger_models::LedgerChange, signed::Signed};
use massa_models::{
    ledger_models::LedgerChanges, Address, Block, BlockHeader, BlockId, EndorsementId, OperationId,
    OperationSearchResult, OperationSearchResultBlockStatus, OperationSearchResultStatus, Slot,
};
use massa_models::{
    prehash::{BuildMap, Map, Set},
    Endorsement,
};
use massa_proof_of_stake_exports::{
    error::ProofOfStakeError, OperationRollInterface, ProofOfStake,
};
use massa_signature::{derive_public_key, PublicKey};
use serde::{Deserialize, Serialize};
use std::mem;
use std::{collections::HashSet, convert::TryInto, usize};
use std::{
    collections::{hash_map, BTreeSet, VecDeque},
    convert::TryFrom,
};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
enum HeaderOrBlock {
    Header(SignedHeader),
    Block(
        Block,
        Map<OperationId, (usize, u64)>,
        Map<EndorsementId, u32>,
    ),
}

impl HeaderOrBlock {
    /// Gets slot for that header or block
    pub fn get_slot(&self) -> Slot {
        match self {
            HeaderOrBlock::Header(header) => header.content.slot,
            HeaderOrBlock::Block(block, ..) => block.header.content.slot,
        }
    }
}

/// Aggregated changes made during a block's execution
#[derive(Debug, Clone)]
pub struct BlockStateAccumulator {
    /// Addresses impacted by ledger updates
    pub loaded_ledger_addrs: Set<Address>,
    /// Subset of the ledger. Contains only data in the thread of the given block
    pub ledger_thread_subset: LedgerSubset,
    /// Cumulative changes made during that block execution
    pub ledger_changes: LedgerChanges,
    /// Addresses impacted by roll updates
    pub loaded_roll_addrs: Set<Address>,
    /// Current roll counts for these addresses
    pub roll_counts: RollCounts,
    /// Roll updates that happened during that block execution
    pub roll_updates: RollUpdates,
    /// Roll updates that happened during current cycle
    pub cycle_roll_updates: RollUpdates,
    /// Cycle of the parent in the same thread
    pub same_thread_parent_cycle: u64,
    /// Address of the parent in the same thread
    pub same_thread_parent_creator: Address,
    /// Addresses of that block endorsers
    pub endorsers_addresses: Vec<Address>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscardReason {
    /// Block is invalid, either structurally, or because of some incompatibility. The String contains the reason for info or debugging.
    Invalid(String),
    /// Block is incompatible with a final block.
    Stale,
    /// Block has enough fitness.
    Final,
}

/// Enum used in blockgraph's state machine
#[derive(Debug, Clone)]
enum BlockStatus {
    /// The block/header has reached consensus but no consensus-level check has been performed.
    /// It will be processed during the next iteration
    Incoming(HeaderOrBlock),
    /// The block/header's slot is too much in the future.
    /// It will be processed at the block/header slot
    WaitingForSlot(HeaderOrBlock),
    /// The block references an unknown Block id
    WaitingForDependencies {
        /// Given header/block
        header_or_block: HeaderOrBlock,
        /// includes self if it's only a header
        unsatisfied_dependencies: Set<BlockId>,
        /// Used to limit and sort the number of blocks/headers wainting for dependencies
        sequence_number: u64,
    },
    /// The block was checked and incluced in the blockgraph
    Active(Box<ActiveBlock>),
    /// The block was discarded and is kept to avoid reprocessing it
    Discarded {
        /// Just the header of that block
        header: SignedHeader,
        /// why it was discarded
        reason: DiscardReason,
        /// Used to limit and sort the number of blocks/headers wainting for dependencies
        sequence_number: u64,
    },
}

/// Block status in the graph that can be exported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExportBlockStatus {
    Incoming,
    WaitingForSlot,
    WaitingForDependencies,
    Active(Block),
    Final(Block),
    Discarded(DiscardReason),
}

impl<'a> From<&'a BlockStatus> for ExportBlockStatus {
    fn from(block: &BlockStatus) -> Self {
        match block {
            BlockStatus::Incoming(_) => ExportBlockStatus::Incoming,
            BlockStatus::WaitingForSlot(_) => ExportBlockStatus::WaitingForSlot,
            BlockStatus::WaitingForDependencies { .. } => ExportBlockStatus::WaitingForDependencies,
            BlockStatus::Active(active_block) => {
                if active_block.is_final {
                    ExportBlockStatus::Final(active_block.block.clone())
                } else {
                    ExportBlockStatus::Active(active_block.block.clone())
                }
            }
            BlockStatus::Discarded { reason, .. } => ExportBlockStatus::Discarded(reason.clone()),
        }
    }
}

/// The block version that can be exported.
/// Note that the detailed list of operation is not exported
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportCompiledBlock {
    /// Header of the corresponding block.
    pub header: SignedHeader,
    /// For (i, set) in children,
    /// set contains the headers' hashes
    /// of blocks referencing exported block as a parent,
    /// in thread i.
    pub children: Vec<Set<BlockId>>,
    /// Active or final
    pub is_final: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    Active,
    Final,
}

impl<'a> BlockGraphExport {
    /// Conversion from blockgraph.
    pub fn extract_from(
        block_graph: &'a BlockGraph,
        slot_start: Option<Slot>,
        slot_end: Option<Slot>,
    ) -> Self {
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
                    if filter(a_block.block.header.content.slot) {
                        export.active_blocks.insert(
                            *hash,
                            ExportCompiledBlock {
                                header: a_block.block.header.clone(),
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

        export
    }
}

#[derive(Debug, Clone)]
pub struct BlockGraphExport {
    /// Genesis blocks.
    pub genesis_blocks: Vec<BlockId>,
    /// Map of active blocks, were blocks are in their exported version.
    pub active_blocks: Map<BlockId, ExportCompiledBlock>,
    /// Finite cache of discarded blocks, in exported version.
    pub discarded_blocks: Map<BlockId, (DiscardReason, SignedHeader)>,
    /// Best parents hashe in each thread.
    pub best_parents: Vec<(BlockId, u64)>,
    /// Latest final period and block hash in each thread.
    pub latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// Head of the incompatibility graph.
    pub gi_head: Map<BlockId, Set<BlockId>>,
    /// List of maximal cliques of compatible blocks.
    pub max_cliques: Vec<Clique>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LedgerDataExport {
    /// Candidate data
    pub candidate_data: LedgerSubset,
    /// Final data
    pub final_data: LedgerSubset,
}

pub struct BlockGraph {
    /// Consensus related config
    cfg: GraphConfig,
    /// Block ids of genesis blocks
    genesis_hashes: Vec<BlockId>,
    /// Used to limit the number of waiting and discarded blocks
    sequence_counter: u64,
    /// Every block we know about
    block_statuses: Map<BlockId, BlockStatus>,
    /// Ids of incomming blocks/headers
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
    /// One (block id, period) per thread TODO not sure I understand the difference with latest_final_blocks_periods
    best_parents: Vec<(BlockId, u64)>,
    /// Incompatibility graph: maps a block id to the block ids it is incompatible with
    /// One entry per Active Block
    pub gi_head: Map<BlockId, Set<BlockId>>,
    /// All the cliques
    max_cliques: Vec<Clique>,
    /// Blocks that need to be propagated
    to_propagate: Map<BlockId, (Block, Set<OperationId>, Vec<EndorsementId>)>,
    /// List of block ids we think are attack attempts
    attack_attempts: Vec<BlockId>,
    /// Newly final blocks
    new_final_blocks: Set<BlockId>,
    /// Newly stale block mapped to creator and slot
    new_stale_blocks: Map<BlockId, (PublicKey, Slot)>,
    /// ledger
    ledger: Ledger,
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
        /// list of (period, address, did_create) for all block/endorsement creation events
        production_events: Vec<(u64, Address, bool)>,
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

/// Possible outcome of block check
#[derive(Debug)]
enum BlockCheckOutcome {
    /// Everything is ok
    Proceed {
        /// one (parent block id, parent's period) per thread
        parents_hash_period: Vec<(BlockId, u64)>,
        /// blocks that block depends on
        dependencies: Set<BlockId>,
        /// blocks that block is incompatible with
        incompatibilities: Set<BlockId>,
        /// number of incompatibilities that are inherited from the parents
        inherited_incompatibilities_count: usize,
        /// changes caused by that block on the ledger
        block_ledger_changes: LedgerChanges,
        /// changes caused by that block on rolls
        roll_updates: RollUpdates,
        /// list of (period, address, did_create) for all block/endorsement creation events
        production_events: Vec<(u64, Address, bool)>,
    },
    /// There is something wrong with that block
    Discard(DiscardReason),
    /// It must wait for its slot to be fully processed
    WaitForSlot,
    /// it must wait for these block ids to be fully processed
    WaitForDependencies(Set<BlockId>),
}

/// Possible outcome of a block's operations check.
#[derive(Debug)]
enum BlockOperationsCheckOutcome {
    /// Everything is ok
    Proceed {
        /// blocks that block depends on
        dependencies: Set<BlockId>,
        /// changes caused by that block on the ledger
        block_ledger_changes: LedgerChanges,
        /// changes caused by that block on rolls
        roll_updates: RollUpdates,
    },
    /// There is something wrong with that batch of operation
    Discard(DiscardReason),
    /// it must wait for these block ids to be fully processed
    WaitForDependencies(Set<BlockId>),
}

/// Creates genesis block in given thread.
///
/// # Arguments
/// * cfg: consensus configuration
/// * serialization_context: ref to a SerializationContext instance
/// * thread_number: thread in wich we want a genesis block
pub fn create_genesis_block(cfg: &GraphConfig, thread_number: u8) -> Result<(BlockId, Block)> {
    let private_key = cfg.genesis_key;
    let public_key = derive_public_key(&private_key);
    let (header_hash, header) = Signed::new_signed(
        BlockHeader {
            creator: public_key,
            slot: Slot::new(0, thread_number),
            parents: Vec::new(),
            operation_merkle_root: Hash::compute_from(&Vec::new()),
            endorsements: Vec::new(),
        },
        &private_key,
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
    pub async fn new(cfg: GraphConfig, init: Option<BootstrapableGraph>) -> Result<Self> {
        // load genesis blocks

        let mut block_statuses = Map::default();
        let mut genesis_block_ids = Vec::with_capacity(cfg.thread_count as usize);
        let ledger_config = LedgerConfig::from(&cfg);
        for thread in 0u8..cfg.thread_count {
            let (block_id, block) = create_genesis_block(&cfg, thread).map_err(|err| {
                GraphError::GenesisCreationError(format!("genesis error {}", err))
            })?;
            genesis_block_ids.push(block_id);
            block_statuses.insert(
                block_id,
                BlockStatus::Active(Box::new(ActiveBlock {
                    creator_address: Address::from_public_key(&block.header.content.creator),
                    parents: Vec::new(),
                    children: vec![Map::default(); cfg.thread_count as usize],
                    dependencies: Set::<BlockId>::default(),
                    descendants: Set::<BlockId>::default(),
                    is_final: true,
                    block_ledger_changes: LedgerChanges::default(), // no changes in genesis blocks
                    operation_set: Default::default(),
                    endorsement_ids: Default::default(),
                    addresses_to_operations: Map::with_capacity_and_hasher(0, BuildMap::default()),
                    roll_updates: RollUpdates::default(), // no roll updates in genesis blocks
                    production_events: vec![],
                    block,
                    addresses_to_endorsements: Default::default(),
                })),
            );
        }

        massa_trace!("consensus.block_graph.new", {});
        if let Some(boot_graph) = init {
            // load from boot graph
            let ledger = Ledger::from_export(
                boot_graph.ledger,
                boot_graph
                    .latest_final_blocks_periods
                    .iter()
                    .map(|(_id, period)| *period)
                    .collect(),
                ledger_config,
            )?;
            let mut res_graph = BlockGraph {
                cfg,
                sequence_counter: 0,
                genesis_hashes: genesis_block_ids,
                active_index: boot_graph.active_blocks.keys().copied().collect(),
                block_statuses: boot_graph
                    .active_blocks
                    .into_iter()
                    .map(|(b_id, block)| {
                        Ok((b_id, BlockStatus::Active(Box::new(block.try_into()?))))
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
                ledger,
                new_final_blocks: Default::default(),
                new_stale_blocks: Default::default(),
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
            let ledger = read_genesis_ledger(&ledger_config).await?;
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
                ledger,
                new_final_blocks: Default::default(),
                new_stale_blocks: Default::default(),
            })
        }
    }

    pub fn export_bootstrap_graph(&self) -> Result<BootstrapableGraph> {
        let required_active_blocks = self.list_required_active_blocks()?;
        let mut active_blocks: Map<BlockId, ExportActiveBlock> =
            Map::with_capacity_and_hasher(required_active_blocks.len(), BuildMap::default());
        for b_id in required_active_blocks {
            if let Some(BlockStatus::Active(a_block)) = self.block_statuses.get(&b_id) {
                active_blocks.insert(b_id, ExportActiveBlock::from(&**a_block));
            } else {
                return Err(GraphError::ContainerInconsistency(format!(
                    "block {} was expected to be active but wasn't on bootstrap graph export",
                    b_id
                )));
            }
        }

        Ok(BootstrapableGraph {
            active_blocks,
            best_parents: self.best_parents.clone(),
            latest_final_blocks_periods: self.latest_final_blocks_periods.clone(),
            gi_head: self.gi_head.clone(),
            max_cliques: self.max_cliques.clone(),
            ledger: LedgerSubset::try_from(&self.ledger)?,
        })
    }

    /// Try to apply an operation in the context of the block
    ///
    /// # Arguments
    /// * state_accu: where the changes are accumulated while we go through the block
    /// * header: the header of the block we are inside
    /// * operation: the operation that we are trying to apply
    /// * pos: proof of stake engine (used for roll related operations)
    pub fn block_state_try_apply_op(
        &self,
        state_accu: &mut BlockStateAccumulator,
        header: &SignedHeader,
        operation: &Signed<Operation, OperationId>,
        pos: &mut ProofOfStake,
    ) -> Result<()> {
        let block_creator_address = Address::from_public_key(&header.content.creator);

        // get roll updates
        let op_roll_updates = operation.content.get_roll_updates()?;
        // get ledger changes (includes fee distribution)
        let op_ledger_changes = operation.content.get_ledger_changes(
            block_creator_address,
            state_accu.endorsers_addresses.clone(),
            state_accu.same_thread_parent_creator,
            self.cfg.roll_price,
            self.cfg.endorsement_count,
        )?;
        // apply to block state accumulator
        self.block_state_try_apply(
            state_accu,
            header,
            Some(op_ledger_changes),
            Some(op_roll_updates),
            pos,
        )?;

        Ok(())
    }

    /// loads missing block state rolls if available
    ///
    /// # Arguments
    /// * accu: accumulated block changes
    /// * header: block header
    /// * pos: proof of state engine
    /// * involved_addrs: involved addresses
    pub fn block_state_sync_rolls(
        &self,
        accu: &mut BlockStateAccumulator,
        header: &SignedHeader,
        pos: &ProofOfStake,
        involved_addrs: &Set<Address>,
    ) -> Result<()> {
        let missing_entries: Set<Address> = involved_addrs
            .difference(&accu.loaded_roll_addrs)
            .copied()
            .collect();
        if !missing_entries.is_empty() {
            let (roll_counts, cycle_roll_updates) = self.get_roll_data_at_parent(
                header.content.parents[header.content.slot.thread as usize],
                Some(&missing_entries),
                pos,
            )?;
            accu.roll_counts.sync_from(involved_addrs, roll_counts);
            let block_cycle = header.content.slot.get_cycle(self.cfg.periods_per_cycle);
            if block_cycle == accu.same_thread_parent_cycle {
                // if the parent cycle is different, ignore cycle roll updates
                accu.cycle_roll_updates
                    .sync_from(involved_addrs, cycle_roll_updates);
            }
        }
        Ok(())
    }

    /// try to apply ledger/roll changes to a block state accumulator
    /// if it fails, the state should remain undisturbed
    pub fn block_state_try_apply(
        &self,
        accu: &mut BlockStateAccumulator,
        header: &SignedHeader,
        mut opt_ledger_changes: Option<LedgerChanges>,
        opt_roll_updates: Option<RollUpdates>,
        pos: &mut ProofOfStake,
    ) -> Result<()> {
        // roll changes
        let (
            applied_roll_addrs,
            loaded_roll_addrs,
            sync_roll_counts,
            sync_roll_updates,
            sync_cycle_roll_updates,
        ) = if let Some(ref roll_updates) = opt_roll_updates {
            // list roll-involved addresses
            let involved_addrs: Set<Address> = roll_updates.get_involved_addresses();

            // get a local copy of the roll_counts and cycle_roll_updates restricted to the involved addresses
            let mut local_roll_counts = accu.roll_counts.clone_subset(&involved_addrs);
            let mut local_cycle_roll_updates =
                accu.cycle_roll_updates.clone_subset(&involved_addrs);

            // load missing entries
            let missing_entries: Set<Address> = involved_addrs
                .difference(&accu.loaded_roll_addrs)
                .copied()
                .collect();
            if !missing_entries.is_empty() {
                let (roll_counts, cycle_roll_updates) = self.get_roll_data_at_parent(
                    header.content.parents[header.content.slot.thread as usize],
                    Some(&missing_entries),
                    pos,
                )?;
                local_roll_counts.sync_from(&involved_addrs, roll_counts);
                let block_cycle = header.content.slot.get_cycle(self.cfg.periods_per_cycle);
                if block_cycle == accu.same_thread_parent_cycle {
                    // if the parent cycle is different, ignore cycle roll updates
                    local_cycle_roll_updates.sync_from(&involved_addrs, cycle_roll_updates);
                }
            }

            // try to apply updates to the local roll_counts
            local_roll_counts.apply_updates(roll_updates)?;

            // try to apply updates to the local cycle_roll_updates
            let compensations = local_cycle_roll_updates.chain(roll_updates)?;

            // credit cycle roll compensations as ledger changes
            if !compensations.is_empty() {
                if opt_ledger_changes.is_none() {
                    // if no ledger changes, create some
                    opt_ledger_changes = Some(Default::default());
                }
                if let Some(ref mut ledger_changes) = opt_ledger_changes {
                    for (addr, compensation) in compensations {
                        let balance_delta = self
                            .cfg
                            .roll_price
                            .checked_mul_u64(compensation.0)
                            .ok_or_else(|| {
                                GraphError::InvalidLedgerChange(
                                    "overflow getting compensated roll credit amount".into(),
                                )
                            })?;
                        ledger_changes.apply(
                            &addr,
                            &LedgerChange {
                                balance_delta,
                                balance_increment: true,
                            },
                        )?;
                    }
                }
            }

            // get a local copy of the block roll updates
            let mut local_roll_updates = accu.roll_updates.clone_subset(&involved_addrs);

            // try to apply updates to the local roll updates
            local_roll_updates.chain(roll_updates)?;

            (
                involved_addrs,
                missing_entries,
                local_roll_counts,
                local_roll_updates,
                local_cycle_roll_updates,
            )
        } else {
            (
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
            )
        };

        // ledger changes
        let (
            applied_ledger_addrs,
            applied_thread_ledger_addrs,
            loaded_ledger_addrs,
            sync_thread_ledger_subset,
            sync_ledger_changes,
        ) = if let Some(ref mut ledger_changes) = opt_ledger_changes {
            // list involved addresses
            let involved_addrs = ledger_changes.get_involved_addresses();
            let thread_addrs: Set<Address> = involved_addrs
                .iter()
                .filter(|addr| addr.get_thread(self.cfg.thread_count) == header.content.slot.thread)
                .copied()
                .collect();

            // get a local copy of the changes involved in the block's thread
            let thread_changes = ledger_changes.clone_subset(&thread_addrs);

            // get a local copy of a block state ledger subset restricted to the involved addresses within the block's thread
            let mut local_ledger_thread_subset =
                accu.ledger_thread_subset.clone_subset(&thread_addrs);

            // load missing addresses into the local thread ledger subset
            let missing_entries: Set<Address> = thread_addrs
                .difference(&accu.loaded_ledger_addrs)
                .copied()
                .collect();
            if !missing_entries.is_empty() {
                let missing_subset =
                    self.get_ledger_at_parents(&header.content.parents, &missing_entries)?;
                local_ledger_thread_subset.sync_from(&missing_entries, missing_subset);
            }

            // try to apply changes to the local thread ledger subset
            local_ledger_thread_subset.apply_changes(&thread_changes)?;

            // get a local copy of the block's ledger changes for the involved addresses
            let mut local_ledger_changes = accu.ledger_changes.clone_subset(&involved_addrs);

            // try to chain changes to the local ledger changes
            local_ledger_changes.chain(ledger_changes)?;
            (
                involved_addrs,
                thread_addrs,
                missing_entries,
                local_ledger_thread_subset,
                local_ledger_changes,
            )
        } else {
            (
                Default::default(),
                Default::default(),
                Default::default(),
                LedgerSubset::default(),
                Default::default(),
            )
        };

        // sync ledger structures
        if opt_ledger_changes.is_some() {
            accu.loaded_ledger_addrs.extend(loaded_ledger_addrs);
            accu.ledger_thread_subset
                .sync_from(&applied_thread_ledger_addrs, sync_thread_ledger_subset);
            accu.ledger_changes
                .sync_from(&applied_ledger_addrs, sync_ledger_changes);
        }

        // sync roll structures
        if opt_roll_updates.is_some() {
            accu.loaded_roll_addrs.extend(loaded_roll_addrs);
            accu.roll_counts
                .sync_from(&applied_roll_addrs, sync_roll_counts);
            accu.roll_updates
                .sync_from(&applied_roll_addrs, sync_roll_updates);
            accu.cycle_roll_updates
                .sync_from(&applied_roll_addrs, sync_cycle_roll_updates);
        }

        Ok(())
    }

    /// initializes a block state accumulator from a block header
    pub fn block_state_accumulator_init(
        &self,
        header: &SignedHeader,
        pos: &mut ProofOfStake,
    ) -> Result<BlockStateAccumulator> {
        let block_thread = header.content.slot.thread;
        let block_cycle = header.content.slot.get_cycle(self.cfg.periods_per_cycle);
        let block_creator_address = Address::from_public_key(&header.content.creator);

        // get same thread parent cycle
        let same_thread_parent = &self
            .get_active_block(&header.content.parents[block_thread as usize])
            .ok_or(GraphError::MissingBlock)?
            .block;

        let same_thread_parent_cycle = same_thread_parent
            .header
            .content
            .slot
            .get_cycle(self.cfg.periods_per_cycle);

        let same_thread_parent_creator =
            Address::from_public_key(&same_thread_parent.header.content.creator);

        let endorsers_addresses: Vec<Address> = header
            .content
            .endorsements
            .iter()
            .map(|ed| Address::from_public_key(&ed.content.sender_public_key))
            .collect();

        // init block state accumulator
        let mut accu = BlockStateAccumulator {
            loaded_ledger_addrs: Set::<Address>::default(),
            ledger_thread_subset: Default::default(),
            ledger_changes: Default::default(),
            loaded_roll_addrs: Set::<Address>::default(),
            roll_counts: Default::default(),
            roll_updates: Default::default(),
            cycle_roll_updates: Default::default(),
            same_thread_parent_cycle,
            same_thread_parent_creator,
            endorsers_addresses: endorsers_addresses.clone(),
        };

        // block constant ledger reward
        let mut reward_ledger_changes = LedgerChanges::default();
        reward_ledger_changes.add_reward(
            block_creator_address,
            endorsers_addresses,
            same_thread_parent_creator,
            self.cfg.block_reward,
            self.cfg.endorsement_count,
        )?;

        self.block_state_try_apply(&mut accu, header, Some(reward_ledger_changes), None, pos)?;

        // apply roll lock funds release
        if accu.same_thread_parent_cycle != block_cycle {
            let mut roll_unlock_ledger_changes = LedgerChanges::default();

            // credit addresses that sold a roll after a lock cycle
            // (step 5.1 in consensus/pos.md)
            for (addr, amount) in pos.get_roll_sell_credit(block_cycle, block_thread)? {
                roll_unlock_ledger_changes.apply(
                    &addr,
                    &LedgerChange {
                        balance_delta: amount,
                        balance_increment: true,
                    },
                )?;
            }

            // apply to block state
            self.block_state_try_apply(
                &mut accu,
                header,
                Some(roll_unlock_ledger_changes),
                None,
                pos,
            )?;
        }

        // apply roll deactivation
        // (step 5.2 in pos.md)
        if accu.same_thread_parent_cycle != block_cycle {
            let mut roll_updates = RollUpdates::default();

            // get addresses for which to deactivate rolls
            let deactivate_addrs = pos.get_roll_deactivations(block_cycle, block_thread)?;

            // load missing address info (because we need to read roll counts)
            self.block_state_sync_rolls(&mut accu, header, pos, &deactivate_addrs)?;

            // accumulate roll updates
            for addr in deactivate_addrs {
                let roll_count = accu.roll_counts.0.get(&addr).unwrap_or(&0);
                roll_updates.apply(
                    &addr,
                    &RollUpdate {
                        roll_purchases: 0,
                        roll_sales: *roll_count,
                    },
                )?;
            }

            // apply changes to block state
            self.block_state_try_apply(&mut accu, header, None, Some(roll_updates), pos)?;
        }

        Ok(accu)
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

    /// for algo see pos.md
    /// if addrs_opt is Some(addrs), restrict to addrs. If None, return all addresses.
    /// returns (roll_counts, cycle_roll_updates)
    pub fn get_roll_data_at_parent(
        &self,
        block_id: BlockId,
        addrs_opt: Option<&Set<Address>>,
        pos: &ProofOfStake,
    ) -> Result<(RollCounts, RollUpdates)> {
        // get target block and its cycle/thread
        let (target_cycle, target_thread) = match self.block_statuses.get(&block_id) {
            Some(BlockStatus::Active(a_block)) => (
                a_block
                    .block
                    .header
                    .content
                    .slot
                    .get_cycle(self.cfg.periods_per_cycle),
                a_block.block.header.content.slot.thread,
            ),
            _ => {
                return Err(GraphError::ContainerInconsistency(format!(
                    "block missing or non-active: {}",
                    block_id
                )));
            }
        };

        // stack back to latest final slot
        // (step 1 in pos.md)
        let mut stack = Vec::new();
        let mut cur_block_id = block_id;
        let final_cycle;
        loop {
            // get block
            let cur_a_block = match self.block_statuses.get(&cur_block_id) {
                Some(BlockStatus::Active(a_block)) => a_block,
                _ => {
                    return Err(GraphError::ContainerInconsistency(format!(
                        "block missing or non-active: {}",
                        cur_block_id
                    )));
                }
            };
            if cur_a_block.is_final {
                // filters out genesis and final blocks
                // (step 1.1 in pos.md)
                final_cycle = cur_a_block
                    .block
                    .header
                    .content
                    .slot
                    .get_cycle(self.cfg.periods_per_cycle);
                break;
            }
            // (step 1.2 in pos.md)
            stack.push(cur_block_id);
            cur_block_id = cur_a_block.parents[target_thread as usize].0;
        }

        // get latest final PoS state for addresses
        // (step 2 a,d 3 in pos.md)
        let (mut cur_rolls, mut cur_cycle_roll_updates) = {
            // (step 2 in pos.md)
            let cycle_state = pos
                .get_final_roll_data(final_cycle, target_thread)
                .ok_or_else(|| {
                    GraphError::ContainerInconsistency(format!(
                        "final PoS cycle not available: {}",
                        final_cycle
                    ))
                })?;
            // (step 3 in pos.md)
            let cur_cycle_roll_updates = if final_cycle == target_cycle {
                if let Some(addrs) = addrs_opt {
                    cycle_state.cycle_updates.clone_subset(addrs)
                } else {
                    cycle_state.cycle_updates.clone()
                }
            } else {
                RollUpdates::default()
            };
            let cur_rolls = if let Some(addrs) = addrs_opt {
                cycle_state.roll_count.clone_subset(addrs)
            } else {
                cycle_state.roll_count.clone()
            };
            (cur_rolls, cur_cycle_roll_updates)
        };

        // unstack blocks and apply their roll changes
        // (step 4 in pos.md)
        while let Some(cur_block_id) = stack.pop() {
            // get block and apply its roll updates to cur_rolls and cur_cycle_roll_updates if in the same cycle as the target block
            match self.block_statuses.get(&cur_block_id) {
                Some(BlockStatus::Active(a_block)) => {
                    let applied_updates = if let Some(addrs) = addrs_opt {
                        a_block.roll_updates.clone_subset(addrs)
                    } else {
                        a_block.roll_updates.clone()
                    };
                    // (step 4.1 in pos.md)
                    cur_rolls.apply_updates(&applied_updates)?;
                    // (step 4.2 in pos.md)
                    if a_block
                        .block
                        .header
                        .content
                        .slot
                        .get_cycle(self.cfg.periods_per_cycle)
                        == target_cycle
                    {
                        // if the block is in the target cycle, accumulate the roll updates
                        // applies compensations but ignores their amount
                        cur_cycle_roll_updates.chain(&applied_updates)?;
                    }
                }
                _ => {
                    return Err(GraphError::ContainerInconsistency(format!(
                        "block missing or non-active: {}",
                        cur_block_id
                    )));
                }
            };
        }

        // (step 5 in pos.md)
        Ok((cur_rolls, cur_cycle_roll_updates))
    }

    /// gets Ledger data export for given Addressees
    pub fn get_ledger_data_export(&self, addresses: &Set<Address>) -> Result<LedgerDataExport> {
        let best_parents = self.get_best_parents();
        Ok(LedgerDataExport {
            candidate_data: self.get_ledger_at_parents(
                &best_parents
                    .iter()
                    .map(|(b, _p)| *b)
                    .collect::<Vec<BlockId>>(),
                addresses,
            )?,
            final_data: self.ledger.get_final_ledger_subset(addresses)?,
        })
    }

    pub fn get_operations_involving_address(
        &self,
        address: &Address,
    ) -> Result<Map<OperationId, OperationSearchResult>> {
        let mut res: Map<OperationId, OperationSearchResult> = Default::default();
        'outer: for b_id in self.active_index.iter() {
            if let Some(BlockStatus::Active(active_block)) = self.block_statuses.get(b_id) {
                if let Some(ops) = active_block.addresses_to_operations.get(address) {
                    for op in ops.iter() {
                        let (idx, _) = active_block.operation_set.get(op).ok_or_else(|| {
                            GraphError::ContainerInconsistency(format!("op {} should be here", op))
                        })?;
                        let search = OperationSearchResult {
                            op: active_block.block.operations[*idx].clone(),
                            in_pool: false,
                            in_blocks: vec![(
                                active_block.block.header.content.compute_id()?,
                                (*idx, active_block.is_final),
                            )]
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
    /// * block_id : block ID
    pub fn get_active_block(&self, block_id: &BlockId) -> Option<&ActiveBlock> {
        BlockGraph::get_full_active_block(&self.block_statuses, *block_id)
    }

    pub fn get_export_block_status(&self, block_id: &BlockId) -> Option<ExportBlockStatus> {
        self.block_statuses
            .get(block_id)
            .map(|block_status| block_status.into())
    }

    /// Retrieves operations from operation Ids
    pub fn get_operations(
        &self,
        operation_ids: &Set<OperationId>,
    ) -> Map<OperationId, OperationSearchResult> {
        let mut res: Map<OperationId, OperationSearchResult> = Default::default();
        // for each active block
        for block_id in self.active_index.iter() {
            if let Some(BlockStatus::Active(active_block)) = self.block_statuses.get(block_id) {
                // check the intersection with the wanted operation ids, and update/insert into results
                operation_ids
                    .iter()
                    .filter_map(|op_id| {
                        active_block
                            .operation_set
                            .get(op_id)
                            .map(|(idx, _)| (op_id, idx, &active_block.block.operations[*idx]))
                    })
                    .for_each(|(op_id, idx, op)| {
                        let search_new = OperationSearchResult {
                            op: op.clone(),
                            in_pool: false,
                            in_blocks: vec![(*block_id, (*idx, active_block.is_final))]
                                .into_iter()
                                .collect(),
                            status: OperationSearchResultStatus::InBlock(
                                OperationSearchResultBlockStatus::Active,
                            ),
                        };
                        res.entry(*op_id)
                            .and_modify(|search_old| search_old.extend(&search_new))
                            .or_insert(search_new);
                    });
            }
        }
        res
    }

    /// signal new slot
    pub fn slot_tick(&mut self, pos: &mut ProofOfStake, current_slot: Option<Slot>) -> Result<()> {
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
        self.rec_process(to_process, pos, current_slot)?;

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
        header: SignedHeader,
        pos: &mut ProofOfStake,
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
        self.rec_process(to_ack, pos, current_slot)?;

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
        block: Block,
        operation_set: Map<OperationId, (usize, u64)>,
        endorsement_ids: Map<EndorsementId, u32>,
        pos: &mut ProofOfStake,
        current_slot: Option<Slot>,
    ) -> Result<()> {
        // ignore genesis blocks
        if self.genesis_hashes.contains(&block_id) {
            return Ok(());
        }
        debug!(
            "received block {} for slot {}",
            block_id, block.header.content.slot
        );
        massa_trace!("consensus.block_graph.incoming_block", {"block_id": block_id, "block": block});
        let mut to_ack: BTreeSet<(Slot, BlockId)> = BTreeSet::new();
        match self.block_statuses.entry(block_id) {
            // if absent => add as Incoming, call rec_ack on it
            hash_map::Entry::Vacant(vac) => {
                to_ack.insert((block.header.content.slot, block_id));
                vac.insert(BlockStatus::Incoming(HeaderOrBlock::Block(
                    block,
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
                    *header_or_block = HeaderOrBlock::Block(block, operation_set, endorsement_ids);
                }
                BlockStatus::WaitingForDependencies {
                    header_or_block,
                    unsatisfied_dependencies,
                    ..
                } => {
                    // promote to full block and satisfy self-dependency
                    if unsatisfied_dependencies.remove(&block_id) {
                        // a dependency was satisfied: process
                        to_ack.insert((block.header.content.slot, block_id));
                    }
                    *header_or_block = HeaderOrBlock::Block(block, operation_set, endorsement_ids);
                    // promote in dependencies
                    self.promote_dep_tree(block_id)?;
                }
                _ => return Ok(()),
            },
        }

        // process
        self.rec_process(to_ack, pos, current_slot)?;

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
        pos: &mut ProofOfStake,
        current_slot: Option<Slot>,
    ) -> Result<()> {
        // order processing by (slot, hash)
        while let Some((_slot, hash)) = to_ack.pop_first() {
            to_ack.extend(self.process(hash, pos, current_slot)?)
        }
        Ok(())
    }

    /// ack a single item, return a set of items to re-ack
    ///
    /// Checks performed:
    /// - See `check_header` for checks on incoming headers.
    /// - See `check_block` for checks on incoming blocks.
    fn process(
        &mut self,
        block_id: BlockId,
        pos: &mut ProofOfStake,
        current_slot: Option<Slot>,
    ) -> Result<BTreeSet<(Slot, BlockId)>> {
        // list items to reprocess
        let mut reprocess = BTreeSet::new();

        massa_trace!("consensus.block_graph.process", { "block_id": block_id });
        // control all the waiting states and try to get a valid block
        let (
            valid_block,
            valid_block_parents_hash_period,
            valid_block_deps,
            valid_block_incomp,
            valid_block_inherited_incomp_count,
            valid_block_changes,
            valid_block_operation_set,
            valid_block_endorsement_ids,
            valid_block_roll_updates,
            valid_block_production_events,
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
                match self.check_header(&block_id, &header, pos, current_slot)? {
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
                                .insert(block_id, (header.content.creator, header.content.slot));
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
                let (block, operation_set, endorsement_ids) = if let Some(BlockStatus::Incoming(
                    HeaderOrBlock::Block(block, operation_set, endorsement_ids),
                )) =
                    self.block_statuses.remove(&block_id)
                {
                    self.incoming_index.remove(&block_id);
                    (block, operation_set, endorsement_ids)
                } else {
                    return Err(GraphError::ContainerInconsistency(format!(
                        "inconsistency inside block statuses removing incoming block {}",
                        block_id
                    )));
                };
                match self.check_block(&block_id, &block, &operation_set, pos, current_slot)? {
                    BlockCheckOutcome::Proceed {
                        parents_hash_period,
                        dependencies,
                        incompatibilities,
                        inherited_incompatibilities_count,
                        block_ledger_changes,
                        roll_updates,
                        production_events,
                    } => {
                        // block is valid: remove it from Incoming and return it
                        massa_trace!("consensus.block_graph.process.incoming_block.valid", {
                            "block_id": block_id
                        });
                        (
                            block,
                            parents_hash_period,
                            dependencies,
                            incompatibilities,
                            inherited_incompatibilities_count,
                            block_ledger_changes,
                            operation_set,
                            endorsement_ids,
                            roll_updates,
                            production_events,
                        )
                    }
                    BlockCheckOutcome::WaitForDependencies(dependencies) => {
                        // set as waiting dependencies
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::WaitingForDependencies {
                                header_or_block: HeaderOrBlock::Block(
                                    block,
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
                    BlockCheckOutcome::WaitForSlot => {
                        // set as waiting for slot
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::WaitingForSlot(HeaderOrBlock::Block(
                                block,
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
                    BlockCheckOutcome::Discard(reason) => {
                        self.maybe_note_attack_attempt(&reason, &block_id);
                        massa_trace!("consensus.block_graph.process.incoming_block.discarded", {"block_id": block_id, "reason": reason});
                        // count stales
                        if reason == DiscardReason::Stale {
                            self.new_stale_blocks.insert(
                                block_id,
                                (block.header.content.creator, block.header.content.slot),
                            );
                        }
                        // add to discard
                        self.block_statuses.insert(
                            block_id,
                            BlockStatus::Discarded {
                                header: block.header,
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

        let valid_block_addresses_to_operations =
            valid_block.involved_addresses(&valid_block_operation_set)?;
        let valid_block_addresses_to_endorsements =
            valid_block.addresses_to_endorsements(&valid_block_endorsement_ids)?;

        // add block to graph
        self.add_block_to_graph(
            block_id,
            valid_block_parents_hash_period,
            valid_block,
            valid_block_deps,
            valid_block_incomp,
            valid_block_inherited_incomp_count,
            valid_block_changes,
            valid_block_operation_set,
            valid_block_endorsement_ids,
            valid_block_addresses_to_operations,
            valid_block_addresses_to_endorsements,
            valid_block_roll_updates,
            valid_block_production_events,
        )?;

        // if the block was added, update linked dependencies and mark satisfied ones for recheck
        if let Some(BlockStatus::Active(active)) = self.block_statuses.get(&block_id) {
            massa_trace!("consensus.block_graph.process.is_active", {
                "block_id": block_id
            });
            self.to_propagate.insert(
                block_id,
                (
                    active.block.clone(),
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

    /// Gets whole ActiveBlock corresponding to given block_id
    ///
    /// # Argument
    /// * block_id : block ID
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
    ///   as determined by the config `future_block_processing_max_periods`.
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
        header: &SignedHeader,
        pos: &mut ProofOfStake,
        current_slot: Option<Slot>,
    ) -> Result<HeaderCheckOutcome> {
        massa_trace!("consensus.block_graph.check_header", {
            "block_id": block_id
        });
        let mut parents: Vec<(BlockId, u64)> = Vec::with_capacity(self.cfg.thread_count as usize);
        let mut deps = Set::<BlockId>::default();
        let mut incomp = Set::<BlockId>::default();
        let mut missing_deps = Set::<BlockId>::default();
        let creator_addr = Address::from_public_key(&header.content.creator);

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
        let slot_draw_address = match pos.draw_block_producer(header.content.slot) {
            Ok(draws) => draws,
            Err(ProofOfStakeError::PosCycleUnavailable(_)) => {
                // slot is not available yet
                return Ok(HeaderCheckOutcome::WaitForSlot);
            }
            Err(err) => return Err(err.into()),
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
                    if parent.block.header.content.slot.thread != parent_thread
                        || parent.block.header.content.slot >= header.content.slot
                    {
                        return Ok(HeaderCheckOutcome::Discard(DiscardReason::Invalid(
                            format!(
                                "Bad parent {} in thread:{} or slot:{} for {}.",
                                parent_hash,
                                parent_thread,
                                parent.block.header.content.slot,
                                header.content.slot
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

                    parents.push((parent_hash, parent.block.header.content.slot.period));
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
                            if gp.block.header.content.slot.period > gp_max_slots[gp_i as usize] {
                                if gp_i < parent_i {
                                    return Ok(HeaderCheckOutcome::Discard(
                                        DiscardReason::Invalid(
                                            "grandpa error: gp_i < parent_i".to_string(),
                                        ),
                                    ));
                                }
                                gp_max_slots[gp_i as usize] = gp.block.header.content.slot.period;
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
        match self.check_endorsements(header, pos, parent_in_own_thread)? {
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

                // check if the parent in tauB has a strictly lower period number than B's parent in tauB
                // note: cur_b cannot be genesis at gen > 1
                if BlockGraph::get_full_active_block(
                    &self.block_statuses,
                    cur_b.block.header.content.parents[header.content.slot.thread as usize],
                )
                .ok_or_else(||
                    GraphError::ContainerInconsistency(
                        format!("inconsistency inside block statuses searching {} check if the parent in tauB has a strictly lower period number than B's parent in tauB while checking grandpa incompatibility of block {}",
                        cur_b.block.header.content.parents[header.content.slot.thread as usize],
                        block_id)
                    ))?
                .block
                .header
                .content
                .slot
                .period
                    < parent_in_own_thread.block.header.content.slot.period
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

        // list production events
        let mut production_events = vec![(header.content.slot.period, creator_addr, true)];
        for miss_period in
            (parents[header.content.slot.thread as usize].1 + 1)..header.content.slot.period
        {
            let miss_slot = Slot::new(miss_period, header.content.slot.thread);
            let slot_draw_address = match pos.draw_block_producer(miss_slot) {
                Ok(draws) => draws,
                Err(ProofOfStakeError::PosCycleUnavailable(_)) => {
                    // slot is not available yet
                    return Ok(HeaderCheckOutcome::WaitForSlot);
                }
                Err(err) => return Err(err.into()),
            };
            production_events.push((miss_period, slot_draw_address, false));
        }

        Ok(HeaderCheckOutcome::Proceed {
            parents_hash_period: parents,
            dependencies: deps,
            incompatibilities: incomp,
            inherited_incompatibilities_count: inherited_incomp_count,
            production_events,
        })
    }

    /// check endorsements:
    /// * endorser was selected for that (slot, index)
    /// * endorsed slot is parent_in_own_thread slot
    fn check_endorsements(
        &self,
        header: &SignedHeader,
        pos: &mut ProofOfStake,
        parent_in_own_thread: &ActiveBlock,
    ) -> Result<EndorsementsCheckOutcome> {
        // check endorsements
        let endorsement_draws =
            match pos.draw_endorsement_producers(parent_in_own_thread.block.header.content.slot) {
                Ok(draws) => draws,
                Err(ProofOfStakeError::PosCycleUnavailable(_)) => {
                    // slot is not available yet
                    return Ok(EndorsementsCheckOutcome::WaitForSlot);
                }
                Err(err) => return Err(err.into()),
            };
        for endorsement in header.content.endorsements.iter() {
            // check that the draw is correct
            if Address::from_public_key(&endorsement.content.sender_public_key)
                != endorsement_draws[endorsement.content.index as usize]
            {
                return Ok(EndorsementsCheckOutcome::Discard(DiscardReason::Invalid(
                    format!(
                        "endorser draw mismatch for header in slot: {}",
                        header.content.slot
                    ),
                )));
            }
            // check that the endorsement slot matches the endorsed block
            if endorsement.content.slot != parent_in_own_thread.block.header.content.slot {
                return Ok(EndorsementsCheckOutcome::Discard(DiscardReason::Invalid(
                    format!("endorsement targets a block with wrong slot. Block's parent: {}, endorsement: {}",
                            parent_in_own_thread.block.header.content.slot, endorsement.content.slot),
                )));
            }

            // note that the following aspects are checked in protocol
            // * signature
            // * intra block endorsement reuse
            // * intra block index reuse
            // * slot in the same thread as block's slot
            // * slot is before the block's slot
            // * the endorsed block is the parent in the same thread
        }

        Ok(EndorsementsCheckOutcome::Proceed)
    }

    /// Process and incoming block.
    ///
    /// Checks performed:
    /// - See `check_header`.
    /// - See `check_operations`.
    fn check_block(
        &self,
        block_id: &BlockId,
        block: &Block,
        operation_set: &Map<OperationId, (usize, u64)>,
        pos: &mut ProofOfStake,
        current_slot: Option<Slot>,
    ) -> Result<BlockCheckOutcome> {
        massa_trace!("consensus.block_graph.check_block", {
            "block_id": block_id
        });
        let mut deps;
        let incomp;
        let parents;
        let inherited_incomp_count;
        let production_evts;

        // check header
        match self.check_header(block_id, &block.header, pos, current_slot)? {
            HeaderCheckOutcome::Proceed {
                parents_hash_period,
                dependencies,
                incompatibilities,
                inherited_incompatibilities_count,
                production_events,
            } => {
                // block_changes can be ignored as it is empty, (maybe add an error if not)
                parents = parents_hash_period;
                deps = dependencies;
                incomp = incompatibilities;
                inherited_incomp_count = inherited_incompatibilities_count;
                production_evts = production_events;
            }
            HeaderCheckOutcome::Discard(reason) => return Ok(BlockCheckOutcome::Discard(reason)),
            HeaderCheckOutcome::WaitForDependencies(deps) => {
                return Ok(BlockCheckOutcome::WaitForDependencies(deps))
            }
            HeaderCheckOutcome::WaitForSlot => return Ok(BlockCheckOutcome::WaitForSlot),
        }

        // check operations
        let (operations_deps, block_ledger_changes, roll_updates) =
            match self.check_operations(block, operation_set, pos)? {
                BlockOperationsCheckOutcome::Proceed {
                    dependencies,
                    block_ledger_changes,
                    roll_updates,
                } => (dependencies, block_ledger_changes, roll_updates),
                BlockOperationsCheckOutcome::Discard(reason) => {
                    println!("Discarding ops: {:?}", reason);
                    return Ok(BlockCheckOutcome::Discard(reason));
                }
                BlockOperationsCheckOutcome::WaitForDependencies(deps) => {
                    return Ok(BlockCheckOutcome::WaitForDependencies(deps))
                }
            };
        deps.extend(operations_deps);

        massa_trace!("consensus.block_graph.check_block.ok", {
            "block_id": block_id
        });

        Ok(BlockCheckOutcome::Proceed {
            parents_hash_period: parents,
            dependencies: deps,
            incompatibilities: incomp,
            inherited_incompatibilities_count: inherited_incomp_count,
            block_ledger_changes,
            roll_updates,
            production_events: production_evts,
        })
    }

    /// Check if operations are consistent.
    ///
    /// Returns changes done by that block to the ledger (one hashmap per thread) and rolls
    /// consensus/pos.md#block-reception-process
    ///
    /// Checks performed:
    /// - Check that ops were not reused in previous blocks.
    fn check_operations(
        &self,
        block_to_check: &Block,
        operation_set: &Map<OperationId, (usize, u64)>,
        pos: &mut ProofOfStake,
    ) -> Result<BlockOperationsCheckOutcome> {
        // check that ops are not reused in previous blocks. Note that in-block reuse was checked in protocol.
        let mut dependencies: Set<BlockId> = Set::<BlockId>::default();
        for operation in block_to_check.operations.iter() {
            // get thread
            let op_thread = Address::from_public_key(&operation.content.sender_public_key)
                .get_thread(self.cfg.thread_count);

            let op_start_validity_period = *operation
                .content
                .get_validity_range(self.cfg.operation_validity_periods)
                .start();

            let mut current_block_id = block_to_check.header.content.parents[op_thread as usize]; // non-genesis => has parents
            loop {
                // get block to process.
                let current_block = match self.block_statuses.get(&current_block_id) {
                    Some(block) => match block {
                        BlockStatus::Active(block) => block,
                        _ => return Err(GraphError::ContainerInconsistency(format!("block {} is not active but is an ancestor of a potentially active block", current_block_id))),
                    },
                    None => {
                        let mut missing_deps = Set::<BlockId>::with_capacity_and_hasher(1, BuildMap::default());
                        missing_deps.insert(current_block_id);
                        return Ok(BlockOperationsCheckOutcome::WaitForDependencies(missing_deps));
                    }
                };

                // stop at op validity start
                if current_block.block.header.content.slot.period < op_start_validity_period {
                    break; // next op.
                }

                // check if present
                if current_block
                    .operation_set
                    .keys()
                    .any(|k| operation_set.contains_key(k))
                {
                    error!("block graph check_operations error, block operation already integrated in another block");
                    return Ok(BlockOperationsCheckOutcome::Discard(
                        DiscardReason::Invalid(
                            "Block operation already integrated in another block".to_string(),
                        ),
                    ));
                }
                dependencies.insert(current_block_id);

                if current_block.parents.is_empty() {
                    // genesis block found
                    break;
                }

                current_block_id = current_block.parents[op_thread as usize].0;
            }
        }

        // initialize block state accumulator
        let mut state_accu = match self.block_state_accumulator_init(&block_to_check.header, pos) {
            Ok(accu) => accu,
            Err(err) => {
                warn!(
                    "block graph check_operations error, could not init block state accumulator: {}",
                    err
                );
                return Ok(BlockOperationsCheckOutcome::Discard(
                    DiscardReason::Invalid(format!("block graph check_operations error, could not init block state accumulator: {}", err)),
                ));
            }
        };

        // all operations
        // (including step 6 in consensus/pos.md)
        for operation in block_to_check.operations.iter() {
            match self.block_state_try_apply_op(
                &mut state_accu,
                &block_to_check.header,
                operation,
                pos,
            ) {
                Ok(_) => (),
                Err(err) => {
                    warn!(
                        "block graph check_operations error, operation apply to state: {}",
                        err
                    );
                    return Ok(BlockOperationsCheckOutcome::Discard(
                        DiscardReason::Invalid(format!(
                            "block graph check_operations error, operation apply to state: {}",
                            err
                        )),
                    ));
                }
            };
        }

        Ok(BlockOperationsCheckOutcome::Proceed {
            dependencies,
            block_ledger_changes: state_accu.ledger_changes,
            roll_updates: state_accu.roll_updates,
        })
    }

    pub fn get_genesis_block_ids(&self) -> &Vec<BlockId> {
        &self.genesis_hashes
    }

    /// Compute ledger subset after given parents for given addresses
    pub fn get_ledger_at_parents(
        &self,
        parents: &[BlockId],
        query_addrs: &Set<Address>,
    ) -> Result<LedgerSubset> {
        // check that all addresses belong to threads with parents later or equal to the latest_final_block of that thread
        let involved_threads: HashSet<u8> = query_addrs
            .iter()
            .map(|addr| addr.get_thread(self.cfg.thread_count))
            .collect();
        for thread in involved_threads.into_iter() {
            match self.block_statuses.get(&parents[thread as usize]) {
                Some(BlockStatus::Active(b)) => {
                    if b.block.header.content.slot.period
                        < self.latest_final_blocks_periods[thread as usize].1
                    {
                        return Err(GraphError::ContainerInconsistency(format!(
                            "asking for operations in thread {}, for which the given parent is older than the latest final block of that thread",
                            thread
                        )));
                    }
                }
                _ => {
                    return Err(GraphError::ContainerInconsistency(format!(
                        "parent block missing or in non-active state: {}",
                        parents[thread as usize]
                    )));
                }
            }
        }

        // compute backtrack ending slots for each thread
        let mut stop_periods =
            vec![vec![0u64; self.cfg.thread_count as usize]; self.cfg.thread_count as usize];
        for target_thread in 0u8..self.cfg.thread_count {
            let (target_last_final_id, target_last_final_period) =
                self.latest_final_blocks_periods[target_thread as usize];
            match self.block_statuses.get(&target_last_final_id) {
                Some(BlockStatus::Active(b)) => {
                    if !b.parents.is_empty() {
                        stop_periods[target_thread as usize] =
                            b.parents.iter().map(|(_id, period)| period + 1).collect();
                    }
                }
                _ => {
                    return Err(GraphError::ContainerInconsistency(format!(
                        "last final block missing or in non-active state: {}",
                        target_last_final_id
                    )));
                }
            }
            stop_periods[target_thread as usize][target_thread as usize] =
                target_last_final_period + 1;
        }

        // backtrack blocks starting from parents
        let mut ancestry = Set::<BlockId>::default();
        let mut to_scan: Vec<BlockId> = parents.to_vec();
        let mut accumulated_changes = LedgerChanges::default();
        while let Some(scan_b_id) = to_scan.pop() {
            // insert into ancestry, ignore if already scanned
            if !ancestry.insert(scan_b_id) {
                continue;
            }

            // get block, quit if not found or not active
            let scan_b = match self.block_statuses.get(&scan_b_id) {
                Some(BlockStatus::Active(b)) => b,
                _ => {
                    return Err(GraphError::ContainerInconsistency(format!(
                        "missing or not active block during ancestry traversal: {}",
                        scan_b_id
                    )));
                }
            };

            // accumulate ledger changes
            // Warning 1: this uses ledger change commutativity and associativity, may not work with smart contracts
            // Warning 2: we assume that overflows cannot happen here (they won't be deterministic)
            let mut explore_parents = false;
            for thread in 0u8..self.cfg.thread_count {
                if scan_b.block.header.content.slot.period
                    < stop_periods[thread as usize]
                        [scan_b.block.header.content.slot.thread as usize]
                {
                    continue;
                }
                explore_parents = true;

                for (addr, change) in scan_b.block_ledger_changes.0.iter() {
                    if query_addrs.contains(addr)
                        && addr.get_thread(self.cfg.thread_count) == thread
                    {
                        accumulated_changes.apply(addr, change)?;
                    }
                }
            }

            // if this ancestor is still useful for the ledger of some thread, explore its parents
            if explore_parents {
                to_scan.extend(scan_b.parents.iter().map(|(id, _period)| id));
            }
        }

        // get final ledger and apply changes to it
        let mut res_ledger = self.ledger.get_final_ledger_subset(query_addrs)?;
        res_ledger.apply_changes(&accumulated_changes)?;

        Ok(res_ledger)
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
        add_block: Block,
        deps: Set<BlockId>,
        incomp: Set<BlockId>,
        inherited_incomp_count: usize,
        block_ledger_changes: LedgerChanges,
        operation_set: Map<OperationId, (usize, u64)>,
        endorsement_ids: Map<EndorsementId, u32>,
        addresses_to_operations: Map<Address, Set<OperationId>>,
        addresses_to_endorsements: Map<Address, Set<EndorsementId>>,
        roll_updates: RollUpdates,
        production_events: Vec<(u64, Address, bool)>,
    ) -> Result<()> {
        massa_trace!("consensus.block_graph.add_block_to_graph", {
            "block_id": add_block_id
        });
        // add block to status structure
        self.block_statuses.insert(
            add_block_id,
            BlockStatus::Active(Box::new(ActiveBlock {
                creator_address: Address::from_public_key(&add_block.header.content.creator),
                parents: parents_hash_period.clone(),
                dependencies: deps,
                descendants: Set::<BlockId>::default(),
                block: add_block.clone(),
                children: vec![Default::default(); self.cfg.thread_count as usize],
                is_final: false,
                block_ledger_changes,
                operation_set,
                endorsement_ids,
                addresses_to_operations,
                roll_updates,
                production_events,
                addresses_to_endorsements,
            })),
        );
        self.active_index.insert(add_block_id);

        // add as child to parents
        for (parent_h, _parent_period) in parents_hash_period.iter() {
            if let Some(BlockStatus::Active(a_parent)) = self.block_statuses.get_mut(parent_h) {
                a_parent.children[add_block.header.content.slot.thread as usize]
                    .insert(add_block_id, add_block.header.content.slot.period);
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
                .ok_or(GraphError::MissingBlock)?
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
                        num::BigInt::from_bytes_be(num::bigint::Sign::Plus, &block_h.to_bytes());
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
                    .block.header.content.slot;
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
                    if let Some(BlockStatus::Active(active_block)) =
                        self.block_statuses.get_mut(parent_h)
                    {
                        active_block.children
                            [active_block.block.header.content.slot.thread as usize]
                            .remove(&stale_block_hash);
                    }
                }

                massa_trace!("consensus.block_graph.add_block_to_graph.stale", {
                    "hash": stale_block_hash
                });
                // mark as stale
                self.new_stale_blocks.insert(
                    stale_block_hash,
                    (
                        active_block.block.header.content.creator,
                        active_block.block.header.content.slot,
                    ),
                );
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
                            .ok_or(GraphError::MissingBlock)?
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

        // Save latest_final_blocks_periods for later use when updating the ledger.
        let old_latest_final_blocks_periods = self.latest_final_blocks_periods.clone();

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
                if final_block.block.header.content.slot.period
                    > self.latest_final_blocks_periods
                        [final_block.block.header.content.slot.thread as usize]
                        .1
                {
                    self.latest_final_blocks_periods
                        [final_block.block.header.content.slot.thread as usize] = (
                        final_block_hash,
                        final_block.block.header.content.slot.period,
                    );
                }
                // update new final blocks list
                self.new_final_blocks.insert(final_block_hash);
            } else {
                return Err(GraphError::ContainerInconsistency(format!("inconsistency inside block statuses updating final blocks adding {} - block {} is missing", add_block_id, final_block_hash)));
            }
        }

        // list threads where latest final block changed
        let changed_threads_old_block_thread_id_period = self
            .latest_final_blocks_periods
            .iter()
            .enumerate()
            .filter_map(|(thread, (b_id, _b_period))| {
                let (old_b_id, old_period) = &old_latest_final_blocks_periods[thread];
                if b_id != old_b_id {
                    return Some((thread as u8, old_b_id, old_period));
                }
                None
            });

        // Update ledger with changes from final blocks, "B2".
        for (changed_thread, old_block_id, old_period) in changed_threads_old_block_thread_id_period
        {
            // Get the old block
            let old_block = match self.block_statuses.get(old_block_id) {
                Some(BlockStatus::Active(latest)) => latest,
                _ => return Err(GraphError::ContainerInconsistency(format!("inconsistency inside block statuses updating final blocks - active old latest final block {} is missing in thread {}", old_block_id, changed_thread))),
            };

            // Get the latest final in the same thread.
            let latest_final_in_thread_id =
                self.latest_final_blocks_periods[changed_thread as usize].0;

            // Init the stop backtrack stop periods
            let mut stop_backtrack_periods = vec![0u64; self.cfg.thread_count as usize];
            for limit_thread in 0u8..self.cfg.thread_count {
                if limit_thread == changed_thread {
                    // in the same thread, set the stop backtrack period to B1.period + 1
                    stop_backtrack_periods[limit_thread as usize] = old_period + 1;
                } else if !old_block.parents.is_empty() {
                    // In every other thread, set it to B1.parents[tau*].period + 1
                    stop_backtrack_periods[limit_thread as usize] =
                        old_block.parents[limit_thread as usize].1 + 1;
                }
            }

            // Backtrack blocks starting from B2.
            let mut ancestry: Set<BlockId> = Set::<BlockId>::default();
            let mut to_scan: Vec<BlockId> = vec![latest_final_in_thread_id]; // B2
            let mut accumulated_changes = LedgerChanges::default();
            while let Some(scan_b_id) = to_scan.pop() {
                // insert into ancestry, ignore if already scanned
                if !ancestry.insert(scan_b_id) {
                    continue;
                }

                // get block, quit if not found or not active
                let scan_b = match self.block_statuses.get(&scan_b_id) {
                    Some(BlockStatus::Active(b)) => b,
                    _ => return Err(GraphError::ContainerInconsistency(format!("inconsistency inside block statuses updating final blocks - block {} is missing", scan_b_id)))
                };

                // accumulate ledger changes
                // Warning 1: this uses ledger change commutativity and associativity, may not work with smart contracts
                // Warning 2: we assume that overflows cannot happen here (they won't be deterministic)
                if scan_b.block.header.content.slot.period
                    < stop_backtrack_periods[scan_b.block.header.content.slot.thread as usize]
                {
                    continue;
                }
                for (addr, change) in scan_b.block_ledger_changes.0.iter() {
                    if addr.get_thread(self.cfg.thread_count) == changed_thread {
                        accumulated_changes.apply(addr, change)?;
                    }
                }

                // Explore parents
                to_scan.extend(
                    scan_b
                        .parents
                        .iter()
                        .map(|(b_id, _period)| *b_id)
                        .collect::<Vec<BlockId>>(),
                );
            }

            // update ledger
            self.ledger.apply_final_changes(
                changed_thread,
                &accumulated_changes,
                self.latest_final_blocks_periods[changed_thread as usize].1,
            )?;
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
                // retain block
                retain_active.insert(current_block_id);

                // stop traversing when reaching a block with period number low enough
                // so that any of its operations will have their validity period expired at the latest final block in thread
                // note: one more is kept because of the way we iterate
                if current_block.block.header.content.slot.period
                    < self.latest_final_blocks_periods[thread]
                        .1
                        .saturating_sub(self.cfg.operation_validity_periods)
                {
                    break;
                }

                // if not genesis, traverse parent
                if current_block.block.header.content.parents.is_empty() {
                    break;
                }

                current_block_id = current_block.block.header.content.parents[thread as usize];
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
                    .block.header
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
                while let Some(c_block) = self.get_active_block(&cursor) {
                    if c_block.block.header.content.slot.period
                        < earliest_retained_periods[thread as usize]
                    {
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
                let (_b_id, latest_final_period) = self.latest_final_blocks_periods
                    [active_block.block.header.content.slot.thread as usize];
                if active_block.block.header.content.slot.period
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
                if let Some(BlockStatus::Active(active_block)) =
                    self.block_statuses.get_mut(parent_h)
                {
                    active_block.children
                        [discarded_active.block.header.content.slot.thread as usize]
                        .remove(&discard_active_h);
                }
            }

            massa_trace!("consensus.block_graph.prune_active", {"hash": discard_active_h, "reason": DiscardReason::Final});
            // mark as final
            self.block_statuses.insert(
                discard_active_h,
                BlockStatus::Discarded {
                    header: discarded_active.block.header.clone(),
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
                    HeaderOrBlock::Block(b, ..) => b.header,
                };
                massa_trace!("consensus.block_graph.prune_waiting_for_dependencies", {"hash": block_id, "reason": reason_opt});

                if let Some(reason) = reason_opt {
                    // add to stats if reason is Stale
                    if reason == DiscardReason::Stale {
                        self.new_stale_blocks
                            .insert(block_id, (header.content.creator, header.content.slot));
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
        (self.cfg.max_future_processing_blocks..slot_waiting.len()).for_each(|idx| {
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
        for (_, block_id) in discard_hashes.into_iter() {
            self.block_statuses.remove(&block_id);
            self.discarded_index.remove(&block_id);
        }
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

    /// get the current block wishlist
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

    pub fn get_clique_count(&self) -> usize {
        self.max_cliques.len()
    }

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
    pub fn clone_all_final_blocks(&self) -> Map<BlockId, Block> {
        self.active_index
            .iter()
            .filter_map(|b_id| {
                if let Some(a_b) = self.get_active_block(b_id) {
                    if a_b.is_final {
                        return Some((*b_id, a_b.block.clone()));
                    }
                }
                None
            })
            .collect()
    }

    /// Get the headers to be propagated.
    /// Must be called by the consensus worker within `block_db_changed`.
    pub fn get_blocks_to_propagate(
        &mut self,
    ) -> Map<BlockId, (Block, Set<OperationId>, Vec<EndorsementId>)> {
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

    pub fn get_endorsement_by_address(
        &self,
        address: Address,
    ) -> Result<Map<EndorsementId, Signed<Endorsement, EndorsementId>>> {
        let mut res: Map<EndorsementId, Signed<Endorsement, EndorsementId>> = Default::default();
        for b_id in self.active_index.iter() {
            if let Some(BlockStatus::Active(ab)) = self.block_statuses.get(b_id) {
                if let Some(eds) = ab.addresses_to_endorsements.get(&address) {
                    for e in ab.block.header.content.endorsements.iter() {
                        let id = e.content.compute_id()?;
                        if eds.contains(&id) {
                            res.insert(id, e.clone());
                        }
                    }
                }
            }
        }
        Ok(res)
    }

    pub fn get_endorsement_by_id(
        &self,
        endorsements: Set<EndorsementId>,
    ) -> Result<Map<EndorsementId, EndorsementInfo>> {
        // iterate on active (final and non-final) blocks

        let mut res = Map::default();
        for block_id in self.active_index.iter() {
            if let Some(BlockStatus::Active(ab)) = self.block_statuses.get(block_id) {
                // list blocks with wanted endorsements
                if endorsements
                    .intersection(&ab.endorsement_ids.keys().copied().collect())
                    .collect::<HashSet<_>>()
                    .is_empty()
                {
                    for e in ab.block.header.content.endorsements.iter() {
                        let id = e.content.compute_id()?;
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
