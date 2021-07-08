//! All information concerning blocks, the block graph and cliques is managed here.
use super::{config::ConsensusConfig, random_selector::RandomSelector};
use crate::error::{BlockAcknowledgeError, ConsensusError};
use crypto::hash::Hash;
use crypto::signature::{Signature, SignatureEngine};
use models::{Block, BlockHeader, BlockHeaderContent, SerializationContext, Slot};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

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

impl<'a> From<&'a CompiledBlock> for ExportCompiledBlock {
    /// Conversion from compiled block
    fn from(block: &'a CompiledBlock) -> Self {
        ExportCompiledBlock {
            block: block.block.header.clone(),
            children: block.children.clone(),
        }
    }
}

/// The discarded blocks structure version that can be exported.
#[derive(Debug, Clone)]
pub struct ExportDiscardedBlocks {
    pub map: HashMap<Hash, (DiscardReason, BlockHeader)>,
}

impl<'a> From<&'a DiscardedBlocks> for ExportDiscardedBlocks {
    /// Conversion from DiscardedBlocks.
    ///
    /// # Argument
    /// * block : DiscardedBlocks to export.
    fn from(block: &'a DiscardedBlocks) -> Self {
        ExportDiscardedBlocks {
            map: block.map.clone(),
        }
    }
}

/// Exprortable verison of the blockGraph
#[derive(Clone, Debug)]
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
    fn from(dbgrah: &'a BlockGraph) -> Self {
        BlockGraphExport {
            genesis_blocks: dbgrah.genesis_blocks.clone(),
            active_blocks: dbgrah
                .active_blocks
                .iter()
                .map(|(hash, block)| (*hash, ExportCompiledBlock::from(block)))
                .collect(), // map of active blocks
            discarded_blocks: ExportDiscardedBlocks::from(&dbgrah.discarded_blocks), // finite cache of discarded blocks
            best_parents: dbgrah.best_parents.clone(), // best parent in each thread
            latest_final_blocks_periods: dbgrah.latest_final_blocks_periods.clone(), // latest final period and block hash in each thread
            gi_head: dbgrah.gi_head.clone(), // head of the incompatibility graph
            max_cliques: dbgrah.max_cliques.clone(), // list of maximal cliques of compatible blocks
        }
    }
}

/// Compliled version of a block.
/// For now, it adds only block's children.
#[derive(Debug, Clone)]
struct CompiledBlock {
    /// Original block ...
    block: Block,
    /// ... and its children.
    children: Vec<HashSet<Hash>>,
}

impl CompiledBlock {
    #[inline]
    /// Computes the fitness of that block.
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

/// Possible discard reasons
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscardReason {
    /// Block is invalid, either structurally, or because of some incompatibility.
    Invalid,
    /// Block is incompatible with a final block.
    Stale,
    /// Block has enough fitness.
    Final,
}

/// Recently discarded blocks' headers are kept here, far away from active blocks.
#[derive(Debug, Clone)]
struct DiscardedBlocks {
    /// Links a hash to corresponding discard reason and block header.
    map: HashMap<Hash, (DiscardReason, BlockHeader)>,
    /// Keeps the order in wich blocks were discarded.
    vec_deque: VecDeque<Hash>,
    /// Maximum number of blocks we keep in memory before definitely forgetting about them.
    max_size: usize,
}

impl DiscardedBlocks {
    /// Creates a new DiscardedBlocks structure.
    fn new(max_size: usize) -> Self {
        DiscardedBlocks {
            map: HashMap::with_capacity(max_size),
            vec_deque: VecDeque::with_capacity(max_size),
            max_size,
        }
    }

    /// Inserts an element at the front of the queue,
    /// if element is already here put it at the front of the queue.
    /// Returns the elements that have been definitely discarded
    /// because of max_size.
    ///
    /// # Argument
    /// * hash : hash of the considered block
    /// * header: header of the block to discard
    /// * reason: why we want it discarded.
    fn insert(
        &mut self,
        hash: Hash,
        header: BlockHeader,
        reason: DiscardReason,
    ) -> Result<HashSet<Hash>, ConsensusError> {
        let mut definitively_discarded = HashSet::new();
        if self.max_size == 0 {
            return Ok(definitively_discarded); // discard pile has zero capacity
        }
        if self.map.insert(hash, (reason, header)).is_none() {
            // newly inserted
            while self.vec_deque.len() > self.max_size - 1 {
                let h = self
                    .vec_deque
                    .pop_back()
                    .ok_or(ConsensusError::ContainerInconsistency)?;
                self.map
                    .remove(&h)
                    .ok_or(ConsensusError::ContainerInconsistency)?;
                definitively_discarded.insert(h);
            }
            self.vec_deque.push_front(hash);
            Ok(definitively_discarded)
        } else {
            // was already present
            let idx = self
                .vec_deque
                .iter()
                .position(|&h| h == hash)
                .ok_or(ConsensusError::ContainerInconsistency)?;
            if idx > 0 {
                self.vec_deque
                    .remove(idx)
                    .ok_or(ConsensusError::ContainerInconsistency)?;
                self.vec_deque.push_front(hash);
            }
            Ok(definitively_discarded)
        }
    }

    /// Checks if element is in discardedblocks
    ///
    /// # Argument
    /// * element: hash of the given block.
    fn contains(&self, element: &Hash) -> bool {
        self.map.contains_key(element)
    }

    /// Gets hash, discard reason and block header for given hash, if it id in the structure.
    ///
    /// # Argument
    /// * element: hash of the given block.
    fn get(&self, element: &Hash) -> Option<(&crypto::hash::Hash, &(DiscardReason, BlockHeader))> {
        self.map.get_key_value(element)
    }
}

/// Here is all information about blocks and graph and cliques.
#[derive(Debug)]
pub struct BlockGraph {
    /// Consensus Configuration
    cfg: ConsensusConfig,
    /// Serialization context
    serialization_context: SerializationContext,
    /// Genesis blocks.
    genesis_blocks: Vec<Hash>,
    /// Map of active blocks.
    active_blocks: HashMap<Hash, CompiledBlock>,
    /// Finite cache of discarded blocks.
    discarded_blocks: DiscardedBlocks,
    /// Best parents hashe in each thread.
    best_parents: Vec<Hash>,
    /// Latest final period and block hash in each thread.
    latest_final_blocks_periods: Vec<(Hash, u64)>,
    /// Head of the incompatibility graph.
    gi_head: HashMap<Hash, HashSet<Hash>>,
    /// List of maximal cliques of compatible blocks.
    max_cliques: Vec<HashSet<Hash>>,
}

/// Creates genesis block in given thread.
///
/// # Arguments
/// * cfg: consensus configuration.
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

/// return type of update_consensus_with_new_block and acknowledge_block
pub struct UpdateConsensusReturn {
    /// Blocks that were pruned during last update
    pub pruned: HashSet<crypto::hash::Hash>,
    /// Final blocks that were discarded that we may want to send to storage
    pub finals: HashMap<crypto::hash::Hash, Block>,
}

impl BlockGraph {
    /// Creates a new block_graph.
    ///
    /// # Argument
    /// * cfg : consensus configuration.
    pub fn new(
        cfg: ConsensusConfig,
        serialization_context: SerializationContext,
    ) -> Result<Self, ConsensusError> {
        let mut active_blocks = HashMap::new();
        let mut block_hashes: Vec<Hash> = Vec::with_capacity(cfg.thread_count as usize);
        for thread in 0u8..cfg.thread_count {
            let (genesis_block_hash, genesis_block) =
                create_genesis_block(&cfg, &serialization_context, thread)
                    .map_err(|_err| ConsensusError::GenesisCreationError)?;
            block_hashes.push(genesis_block_hash);
            active_blocks.insert(
                genesis_block_hash,
                CompiledBlock {
                    block: genesis_block,
                    children: vec![HashSet::new(); cfg.thread_count as usize],
                },
            );
        }
        let max_discarded_blocks = cfg.max_discarded_blocks;
        Ok(BlockGraph {
            cfg,
            serialization_context,
            genesis_blocks: block_hashes.clone(),
            active_blocks,
            discarded_blocks: DiscardedBlocks::new(max_discarded_blocks),
            best_parents: block_hashes.clone(),
            latest_final_blocks_periods: block_hashes.into_iter().map(|h| (h, 0u64)).collect(),
            gi_head: HashMap::new(), // genesis blocks are final and not included in gi_head
            max_cliques: vec![HashSet::new()],
        })
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
        self.active_blocks.get(&hash).map(|cb| &cb.block)
    }

    /// Returns value, or hashes of some of the blocks missing to be conclusive
    ///
    /// # Argument
    /// * parent_hashes : we want to check topological order of parent_hashes.
    fn check_block_parents_topological_order(
        &self,
        parent_hashes: &Vec<Hash>,
    ) -> Result<bool, HashSet<Hash>> {
        let mut gp_max_slots = vec![0u64; self.cfg.thread_count as usize];
        let mut missing: HashSet<Hash> = HashSet::new();
        for parent_i in 0..self.cfg.thread_count {
            let parent_h = parent_hashes[parent_i as usize];
            if let Some(parent) = self.active_blocks.get(&parent_h) {
                if parent.block.header.content.slot.period < gp_max_slots[parent_i as usize] {
                    return Ok(false);
                }
                gp_max_slots[parent_i as usize] = parent.block.header.content.slot.period;
                if parent.block.header.content.slot.period == 0 {
                    // genesis
                    continue;
                }
                for gp_i in 0..self.cfg.thread_count {
                    if gp_i == parent_i {
                        continue;
                    }
                    let gp_h = parent.block.header.content.parents[gp_i as usize];
                    if let Some(gp) = self.active_blocks.get(&gp_h) {
                        if gp.block.header.content.slot.period > gp_max_slots[gp_i as usize] {
                            if gp_i < parent_i {
                                return Ok(false);
                            }
                            gp_max_slots[gp_i as usize] = gp.block.header.content.slot.period;
                        }
                    } else {
                        missing.insert(gp_h);
                    }
                }
            } else {
                missing.insert(parent_h);
            }
        }
        if missing.is_empty() {
            Ok(true)
        } else {
            Err(missing)
        }
    }

    /// Returns hash and resulting discarded blocks
    ///
    /// # Arguments
    /// * val : dummy value used to generate dummy hash
    /// * slot : generated block is in slot slot.
    pub fn create_block(
        &mut self,
        val: String,
        slot: Slot,
    ) -> Result<(Hash, Block), ConsensusError> {
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

    /// Checks a block header
    /// Returns a set of hashes of missing dependencies
    ///
    /// # Arguments
    /// * hash : hash of the given block
    /// * header : header to check
    /// * selector: selector to draw staker for slot
    /// * current_slot: current slot
    pub fn check_header(
        &mut self,
        hash: &Hash,
        header: &BlockHeader,
        selector: &mut RandomSelector,
        current_slot: Slot,
    ) -> Result<HashSet<Hash>, BlockAcknowledgeError> {
        // todo begin move to check header_structure
        // check if we already know about this block
        if let Some((_, (reason, header))) = self.discarded_blocks.get(&hash) {
            let (reason, header) = (*reason, header.clone());
            // get the reason it was first discarded for
            self.discarded_blocks.insert(hash.clone(), header, reason)?; // promote to the front of the discard pile
            return Err(BlockAcknowledgeError::AlreadyDiscarded); // already discarded
        }
        if self.active_blocks.contains_key(&hash) {
            return Err(BlockAcknowledgeError::AlreadyAcknowledged); // already an active block
        }
        if self.genesis_blocks.contains(&hash) {
            return Err(BlockAcknowledgeError::AlreadyAcknowledged); // it's a genesis block
        }

        // basic structural checks
        if header.content.parents.len() != (self.cfg.thread_count as usize)
            || header.content.slot.period == 0
            || header.content.slot.thread >= self.cfg.thread_count
        {
            self.discarded_blocks
                .insert(hash.clone(), header.clone(), DiscardReason::Invalid)?;
            return Err(BlockAcknowledgeError::InvalidFields);
        }

        // check that is newer than the latest final block in that thread
        if header.content.slot.period
            <= self.latest_final_blocks_periods[header.content.slot.thread as usize].1
        {
            self.discarded_blocks
                .insert(hash.clone(), header.clone(), DiscardReason::Invalid)?;
            return Err(BlockAcknowledgeError::TooOld);
        }
        // todo end move to check header structure

        // todo begin move to how_far_in the future
        // check if block slot is too much in the future
        if header.content.slot
            > Slot::new(
                current_slot
                    .period
                    .saturating_add(self.cfg.future_block_processing_max_periods),
                current_slot.thread,
            )
        {
            return Err(BlockAcknowledgeError::TooMuchInTheFuture);
        }
        // todo end move to how far in the future

        // todo begin move to check roll number
        // check if it was the creator's turn to create this block
        // note: do this AFTER TooMuchInTheFuture checks
        //       to avoid doing too many draws to check blocks in the distant future
        if header.content.creator != self.cfg.nodes[selector.draw(header.content.slot) as usize].0 {
            // it was not the creator's turn to create a block for this slot
            self.discarded_blocks
                .insert(hash.clone(), header.clone(), DiscardReason::Invalid)?;
            return Err(BlockAcknowledgeError::DrawMismatch);
        }
        // todo end move to check roll number

        // check if block is in the future: queue it
        // note: do it after testing signature + draw to prevent queue flooding/DoS
        if header.content.slot > current_slot {
            // todo
            // return Err(BlockAcknowledgeError::InTheFuture(block));
        }

        // TODO check if we already have a block for that slot
        // TODO denounce ? see issue #101

        // todo begin move to check parents
        // ensure parents presence and validity
        let mut missing_dependencies = HashSet::new();
        for parent_thread in 0u8..self.cfg.thread_count {
            let parent_hash = header.content.parents[parent_thread as usize];
            if self.discarded_blocks.contains(&parent_hash) {
                self.discarded_blocks.insert(
                    hash.clone(),
                    header.clone(),
                    DiscardReason::Invalid,
                )?;
                return Err(BlockAcknowledgeError::InvalidParents(
                    "discarded parent".to_string(),
                )); // a parent is discarded
            }
            if let Some(parent) = self.active_blocks.get(&parent_hash) {
                // check that the parent is from an earlier slot in the right thread
                if parent.block.header.content.slot.thread != parent_thread
                    || (parent.block.header.content.slot.period, parent_thread)
                        >= (header.content.slot.period, header.content.slot.thread)
                {
                    // a parent is in the wrong thread or has a slot not strictly before the block
                    self.discarded_blocks.insert(
                        hash.clone(),
                        header.clone(),
                        DiscardReason::Invalid,
                    )?;
                    return Err(BlockAcknowledgeError::InvalidParents(
                        "discarded parent".to_string(),
                    )); // a parent is discarded
                }
            } else {
                // a parent is missing
                missing_dependencies.insert(parent_hash);
            }
            // check that the parents are mutually compatible
            {
                let parent_hashes: HashSet<Hash> = header.content.parents.iter().cloned().collect();
                for parent_h in parent_hashes.iter() {
                    if let Some(incomp) = self.gi_head.get(&parent_h) {
                        if !incomp.is_disjoint(&parent_hashes) {
                            // found mutually incompatible parents
                            self.discarded_blocks.insert(
                                hash.clone(),
                                header.clone(),
                                DiscardReason::Invalid,
                            )?;
                            return Err(BlockAcknowledgeError::InvalidParents(
                                "mutually incompatible parents".to_string(),
                            ));
                        }
                    }
                }
            }
        }
        // check that the parents are mutually compatible
        {
            let parent_hashes: HashSet<Hash> = header.content.parents.iter().cloned().collect();
            for parent_h in parent_hashes.iter() {
                if let Some(incomp) = self.gi_head.get(&parent_h) {
                    if !incomp.is_disjoint(&parent_hashes) {
                        // found mutually incompatible parents
                        self.discarded_blocks.insert(
                            hash.clone(),
                            header.clone(),
                            DiscardReason::Invalid,
                        )?;
                        return Err(BlockAcknowledgeError::InvalidParents(
                            "mutually incompatible parents".to_string(),
                        ));
                    }
                }
            }
        }
        // check the topological consistency of the parents
        if missing_dependencies.is_empty() {
            match self.check_block_parents_topological_order(&header.content.parents) {
                Ok(true) => {}
                Ok(false) => {
                    // inconsistent parent topology
                    self.discarded_blocks.insert(
                        hash.clone(),
                        header.clone(),
                        DiscardReason::Invalid,
                    )?;
                    return Err(BlockAcknowledgeError::InvalidParents(
                        "Inconsistent parent topology".to_string(),
                    ));
                }
                Err(missing) => missing_dependencies.extend(missing), // blocks missing to decide on parent consistency
            }
        }

        Ok(missing_dependencies)
        // todo end move to check parents
    }

    /// Acknowledges a block.
    /// Returns discarded blocks
    ///
    /// # Arguments
    /// * hash : hash of the given block
    /// * block : block to acknowledge
    /// * selector: selector to draw staker for slot
    /// * current_slot: current slot
    pub fn acknowledge_block(
        &mut self,
        hash: Hash,
        block: Block,
        selector: &mut RandomSelector,
        current_slot: Slot,
    ) -> Result<UpdateConsensusReturn, BlockAcknowledgeError> {
        massa_trace!("start_ack_new_block", {
            "block": hash,
            "thread": block.header.content.slot.thread,
            "period": block.header.content.slot.period
        });

        // todo begin move to how far in the future
        // check if block is in the future: queue it
        // note: do it after testing signature + draw to prevent queue flooding/DoS
        if block.header.content.slot > current_slot {
            return Err(BlockAcknowledgeError::InTheFuture(block));
        }
        // todo end move to how far in the future

        let missing_dependencies =
            self.check_header(&hash, &block.header, selector, current_slot)?;
        if !missing_dependencies.is_empty() {
            // there are missing dependencies
            if !missing_dependencies.is_disjoint(&self.genesis_blocks.iter().copied().collect()) {
                // some of the missing dependencies are genesis: consider it as badly chosen parents
                return Err(BlockAcknowledgeError::InvalidParents(
                    "depends on a discarded genesis block".to_string(),
                ));
            }
            return Err(BlockAcknowledgeError::MissingDependencies(
                block,
                missing_dependencies,
            ));
        }

        //TODO check that the block is compatible with all its parents (see issue #102)
        // note: this can only fail due to operations

        // note: here we know that the block is valid

        let res = self.update_consensus_with_new_block(hash, block)?;

        massa_trace!("acknowledged", { "block": hash });
        Ok(res)
    }

    /// Gets a block and all its desencants
    ///
    /// # Argument
    /// * hash : hash of the given block
    fn get_block_and_descendants(&self, hash: Hash) -> Result<HashSet<Hash>, ConsensusError> {
        let mut to_visit = vec![hash];
        let mut result: HashSet<Hash> = HashSet::new();
        while let Some(visit_h) = to_visit.pop() {
            if !result.insert(visit_h) {
                continue; // already visited
            }
            self.active_blocks
                .get(&visit_h)
                .ok_or(ConsensusError::MissingBlock)?
                .children
                .iter()
                .for_each(|thread_children| to_visit.extend(thread_children));
        }
        Ok(result)
    }

    /// Computes max cliques of compatible blocks
    fn compute_max_cliques(&mut self) -> Vec<HashSet<Hash>> {
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

    /// Updates the consensus state by taking a new block into account
    /// if ok, returns the hashmap of pruned blocks. and of final pruned blocks
    ///
    /// # Argument
    /// * hash: hash of the given block
    /// * block: new incomming block
    fn update_consensus_with_new_block(
        &mut self,
        hash: Hash,
        block: Block,
    ) -> Result<UpdateConsensusReturn, ConsensusError> {
        // basic checks
        if block.header.content.parents.len() != self.cfg.thread_count as usize
            || block.header.content.slot.period == 0
            || block.header.content.slot.thread >= self.cfg.thread_count
        {
            return Err(ConsensusError::InvalidBlock);
        }
        // todo end move to check header structure

        // list of incompatibilities
        let mut incomp: HashSet<Hash> = HashSet::new();

        // include all parent's incompatibilites
        for parent_h in block.header.content.parents.iter() {
            if let Some(parent_incomp) = self.gi_head.get(parent_h) {
                incomp.extend(parent_incomp);
            }
        }
        // number of inherited incompatibilities
        let inherited_incomp_count = incomp.len();

        // thread incompatibility test
        self.active_blocks
            .get(&block.header.content.parents[block.header.content.slot.thread as usize])
            .ok_or(ConsensusError::MissingBlock)?
            .children[block.header.content.slot.thread as usize]
            .iter()
            .filter(|&sibling_h| *sibling_h != hash)
            .try_for_each(|&sibling_h| {
                incomp.extend(self.get_block_and_descendants(sibling_h)?);
                Result::<(), ConsensusError>::Ok(())
            })?;

        // grandpa incompatibility test
        let parent_period_in_own_thread = self
            .active_blocks
            .get(&block.header.content.parents[block.header.content.slot.thread as usize])
            .ok_or(ConsensusError::MissingBlock)?
            .block
            .header
            .content
            .slot
            .period;

        for tau in (0u8..self.cfg.thread_count).filter(|&t| t != block.header.content.slot.thread) {
            // for each parent in a different thread tau
            // traverse parent's descendance in tau
            let mut to_explore = vec![(0usize, block.header.content.parents[tau as usize])];
            while let Some((cur_gen, cur_h)) = to_explore.pop() {
                let cur_b = self
                    .active_blocks
                    .get(&cur_h)
                    .ok_or(ConsensusError::MissingBlock)?;
                // traverse but do not check up to generation 1
                if cur_gen <= 1 {
                    to_explore.extend(
                        cur_b.children[tau as usize]
                            .iter()
                            .map(|&c_h| (cur_gen + 1, c_h)),
                    );
                    continue;
                }
                // check if the parent in tauB has a strictly lower period number than B's parent in tauB
                // note: cur_b cannot be genesis at gen > 1
                if self
                    .active_blocks
                    .get(
                        &cur_b.block.header.content.parents
                            [block.header.content.slot.thread as usize],
                    )
                    .ok_or(ConsensusError::MissingBlock)?
                    .block
                    .header
                    .content
                    .slot
                    .period
                    < parent_period_in_own_thread
                {
                    // GPI detected
                    incomp.extend(self.get_block_and_descendants(cur_h)?);
                } // otherwise, cur_b and its descendants cannot be GPI with the block: don't traverse
            }
        }
        // TODO operation incompatibility test (see issue #102)

        // check if there are any final blocks in "incomp". If so, discard block as stale
        // note: we use the fact that active_blocks that are not in gi_head are final
        //       (stale blocks are completely discarded)
        if !incomp.is_subset(&self.gi_head.keys().cloned().collect()) {
            // block is incompatible with some final blocks
            let mut pruned = HashSet::with_capacity(1);
            pruned.insert(hash);
            return Ok(UpdateConsensusReturn {
                pruned,
                finals: HashMap::new(),
            });
        }

        // add block to structure
        self.active_blocks.insert(
            hash,
            CompiledBlock {
                block: block.clone(),
                children: vec![HashSet::new(); self.cfg.thread_count as usize],
            },
        );
        for parent_h in block.header.content.parents.iter() {
            self.active_blocks
                .get_mut(parent_h)
                .ok_or(ConsensusError::MissingBlock)?
                .children[block.header.content.slot.thread as usize]
                .insert(hash);
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
                        self.active_blocks
                            .get(block_h)
                            .ok_or(ConsensusError::MissingBlock)?
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
                let block_c = self
                    .active_blocks
                    .get(block_h)
                    .ok_or(ConsensusError::MissingBlock)?;
                if block_c.children[block_c.block.header.content.slot.thread as usize]
                    .is_disjoint(blockclique)
                {
                    self.best_parents[block_c.block.header.content.slot.thread as usize] = *block_h;
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
        info!("stale_blocks:{:?}", stale_blocks);

        // prune stale blocks
        let mut pruned_blocks: HashSet<Hash> = self
            .prune_blocks(stale_blocks, false, true)?
            .keys()
            .copied()
            .collect();

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
                        if !root && !visited.insert(h) {
                            // lazy boolean evaluation
                            continue; // already visited
                        }
                        let b = self
                            .active_blocks
                            .get(&h)
                            .ok_or(ConsensusError::MissingBlock)?;
                        b.children
                            .iter()
                            .for_each(|c_set| stack.extend(c_set.intersection(clique)));
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

        // find latest final blocks
        for final_block_h in final_blocks.iter() {
            let final_block = self
                .active_blocks
                .get(final_block_h)
                .ok_or(ConsensusError::MissingBlock)?;
            if final_block.block.header.content.slot.period
                > self.latest_final_blocks_periods
                    [final_block.block.header.content.slot.thread as usize]
                    .1
            {
                self.latest_final_blocks_periods
                    [final_block.block.header.content.slot.thread as usize] =
                    (*final_block_h, final_block.block.header.content.slot.period);
            }
        }

        let removed_finals = self.prune_blocks(final_blocks.clone(), true, false)?;
        // prune final blocks
        pruned_blocks.extend(removed_finals.keys());

        Ok(UpdateConsensusReturn {
            pruned: pruned_blocks,
            finals: removed_finals,
        })
    }

    /// Prunes blocks from graph.
    ///
    /// # Arguments
    /// * prune_set: Hash of blocks to prune
    /// * prune_from_cliques if we want to prune blocks from cliques
    /// * are_blocks_stale: prune_set blocks are either stale or final. Used to set discard reason
    fn prune_blocks(
        &mut self,
        prune_set: HashSet<Hash>,
        prune_from_cliques: bool,
        are_blocks_stale: bool,
    ) -> Result<HashMap<Hash, Block>, ConsensusError> {
        // pruning
        for discard_h in prune_set.iter() {
            // remove from cliques
            if prune_from_cliques {
                self.max_cliques
                    .iter_mut()
                    .for_each(|c| drop(c.remove(&discard_h)));
                self.max_cliques.retain(|c| !c.is_empty()); // remove empty cliques
                if self.max_cliques.is_empty() {
                    // make sure at least one clique remains
                    self.max_cliques = vec![HashSet::new()];
                }
            }

            // remove from gi_head
            self.gi_head
                .remove(&discard_h)
                .ok_or(ConsensusError::ContainerInconsistency)?
                .into_iter()
                .try_for_each(|other| {
                    self.gi_head
                        .get_mut(&other)
                        .ok_or(ConsensusError::ContainerInconsistency)?
                        .remove(&discard_h);
                    Result::<(), ConsensusError>::Ok(())
                })?;
        }

        // prune self.active_blocks
        // retain gi_head and last final blocks
        let mut retain_active: HashSet<Hash> = self
            .gi_head
            .keys()
            .copied()
            .chain(
                self.latest_final_blocks_periods
                    .iter()
                    .map(|(h, _)| h.clone()),
            )
            .collect();

        // grow with parents & fill thread holes twice
        for _ in 0..1 {
            // retain the parents of the selected blocks
            let retain_clone = retain_active.clone();
            for retain_h in retain_clone.into_iter() {
                retain_active.extend(
                    self.active_blocks
                        .get(&retain_h)
                        .ok_or(ConsensusError::ContainerInconsistency)?
                        .block
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
                let header = &self
                    .active_blocks
                    .get(retain_h)
                    .ok_or(ConsensusError::ContainerInconsistency)?
                    .block
                    .header;
                earliest_retained_periods[header.content.slot.thread as usize] = std::cmp::min(
                    earliest_retained_periods[header.content.slot.thread as usize],
                    header.content.slot.period,
                );
            }

            // fill up from the latest final block back to the earliest for each thread
            for thread in 0..self.cfg.thread_count {
                let mut cursor = self.latest_final_blocks_periods[thread as usize].0;
                while let Some(c_block) = self.active_blocks.get(&cursor) {
                    if c_block.block.header.content.slot.period
                        < earliest_retained_periods[thread as usize]
                    {
                        break;
                    }
                    retain_active.insert(cursor);
                    if c_block.block.header.content.parents.len() < self.cfg.thread_count as usize {
                        // genesis
                        break;
                    }
                    cursor = c_block.block.header.content.parents[thread as usize];
                }
            }
        }

        // TODO keep enough blocks in each thread to test for still-valid, non-reusable transactions
        // see issue #98
        // remove non-kept from active_blocks and add to discard list
        let mut removed: HashMap<Hash, Block> = self
            .active_blocks
            .drain_filter(|h, _| !retain_active.contains(h))
            .map(|(k, v)| (k, v.block))
            .collect();
        for (hash, block) in removed.iter() {
            for parent_hash in block.header.content.parents.iter() {
                if let Some(parent) = self.active_blocks.get_mut(parent_hash) {
                    parent.children[block.header.content.slot.thread as usize].remove(hash);
                }
            }
        }
        // retain only non-genesis removed blocks
        removed.retain(|h, _| !self.genesis_blocks.contains(h));
        // add removed to discarded
        removed.iter().try_for_each(|(h, block)| {
            let reason = if are_blocks_stale {
                if prune_set.contains(h) {
                    DiscardReason::Stale
                } else {
                    DiscardReason::Final
                }
            } else {
                DiscardReason::Final
            };
            self.discarded_blocks
                .insert(h.clone(), block.header.clone(), reason)
                .map(|_| ())
        })?;

        Ok(removed)
    }
}

/// Checks if the signature is ok.
/// In the future, that check should happen while deserializing
pub fn check_signature(
    header: BlockHeader,
    hash: Hash,
    signature: Signature,
) -> Result<bool, ConsensusError> {
    SignatureEngine::new()
        .verify(&hash, &signature, &header.creator)
        .map_err(|e| ConsensusError::from(e))
}

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
}
