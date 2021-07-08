//! All information concerning blocks, the block graph and cliques is managed here.
use super::{config::ConsensusConfig, random_selector::RandomSelector};
use crate::error::ConsensusError;
use crypto::hash::{Hash, HASH_SIZE_BYTES};
use crypto::signature::derive_public_key;
use models::{
    array_from_slice, u8_from_slice, Block, BlockHeader, BlockHeaderContent, BlockId,
    DeserializeCompact, DeserializeVarInt, ModelsError, SerializationContext, SerializeCompact,
    SerializeVarInt, Slot,
};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map, BTreeSet, HashMap, HashSet, VecDeque};
use std::convert::TryInto;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveBlock {
    block: Block,
    parents: Vec<(BlockId, u64)>, // one (hash, period) per thread ( if not genesis )
    children: Vec<HashMap<BlockId, u64>>, // one HashMap<hash, period> per thread (blocks that need to be kept)
    dependencies: HashSet<BlockId>,       // dependencies required for validity check
    descendants: HashSet<BlockId>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportActiveBlock {
    block: Block,
    parents: Vec<(BlockId, u64)>, // one (hash, period) per thread ( if not genesis )
    children: Vec<Vec<(BlockId, u64)>>, // one HashMap<hash, period> per thread (blocks that need to be kept)
    dependencies: Vec<BlockId>,         // dependencies required for validity check
    is_final: bool,
}

impl From<ActiveBlock> for ExportActiveBlock {
    fn from(block: ActiveBlock) -> Self {
        ExportActiveBlock {
            block: block.block,
            parents: block.parents,
            children: block
                .children
                .into_iter()
                .map(|map| map.into_iter().collect())
                .collect(),
            dependencies: block.dependencies.into_iter().collect(),
            is_final: block.is_final,
        }
    }
}

impl<'a> From<ExportActiveBlock> for ActiveBlock {
    fn from(block: ExportActiveBlock) -> Self {
        ActiveBlock {
            block: block.block,
            parents: block.parents,
            children: block
                .children
                .into_iter()
                .map(|map| map.into_iter().collect())
                .collect(),
            dependencies: block.dependencies.into_iter().collect(),
            descendants: HashSet::new(),
            is_final: block.is_final,
        }
    }
}

impl SerializeCompact for ExportActiveBlock {
    fn to_bytes_compact(
        &self,
        context: &SerializationContext,
    ) -> Result<Vec<u8>, models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        //is_final
        if self.is_final {
            res.push(1);
        } else {
            res.push(0);
        }

        //block
        res.extend(self.block.to_bytes_compact(&context)?);

        //parents
        // parents (note: there should be none if slot period=0)
        if self.parents.len() == 0 {
            res.push(0);
        } else {
            res.push(1);
        }
        for (hash, period) in self.parents.iter() {
            res.extend(&hash.to_bytes());
            res.extend(period.to_varint_bytes());
        }

        //children
        let children_count: u32 = self.children.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many children in ActiveBlock: {:?}", err))
        })?;
        res.extend(u32::from(children_count).to_varint_bytes());
        for map in self.children.iter() {
            let map_count: u32 = map.len().try_into().map_err(|err| {
                ModelsError::SerializeError(format!(
                    "too many entry in children map in ActiveBlock: {:?}",
                    err
                ))
            })?;
            res.extend(u32::from(map_count).to_varint_bytes());
            for (hash, period) in map {
                res.extend(&hash.to_bytes());
                res.extend(period.to_varint_bytes());
            }
        }

        //dependencies
        let dependencies_count: u32 = self.dependencies.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many dependencies in ActiveBlock: {:?}", err))
        })?;
        res.extend(u32::from(dependencies_count).to_varint_bytes());
        for dep in self.dependencies.iter() {
            res.extend(&dep.to_bytes());
        }
        Ok(res)
    }
}

impl DeserializeCompact for ExportActiveBlock {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), models::ModelsError> {
        let mut cursor = 0usize;

        //is_final
        let is_final_u8 = u8_from_slice(&buffer)?;
        cursor += 1;
        let is_final = if is_final_u8 == 0 { false } else { true };

        //block
        let (block, delta) = Block::from_bytes_compact(&buffer[cursor..], &context)?;
        cursor += delta;

        // parents
        let has_parents = u8_from_slice(&buffer[cursor..])?;
        cursor += 1;
        let parents = if has_parents == 1 {
            let mut parents: Vec<(BlockId, u64)> =
                Vec::with_capacity(context.parent_count as usize);
            for _ in 0..context.parent_count {
                let parent_h = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += HASH_SIZE_BYTES;
                let (period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                parents.push((parent_h, period));
            }
            parents
        } else if has_parents == 0 {
            Vec::new()
        } else {
            return Err(ModelsError::SerializeError(
                "ActiveBlock from_bytes_compact bad hasparents flags.".into(),
            ));
        };

        //childrens
        let (children_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        if children_count > context.parent_count.into() {
            return Err(ModelsError::DeserializeError(
                "too much threads with children to deserialize".to_string(),
            ));
        }
        cursor += delta;
        let mut children: Vec<Vec<(BlockId, u64)>> = Vec::with_capacity(children_count as usize);
        for _ in 0..(children_count as usize) {
            let (map_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
            if map_count > context.max_bootstrap_children {
                return Err(ModelsError::DeserializeError(
                    "too much children to deserialize".to_string(),
                ));
            }
            cursor += delta;
            let mut map: Vec<(BlockId, u64)> = Vec::with_capacity(map_count as usize);
            for _ in 0..(map_count as usize) {
                let hash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += HASH_SIZE_BYTES;
                let (period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;
                map.push((hash, period));
            }
            children.push(map);
        }

        //dependencies
        let (dependencies_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        if dependencies_count > context.max_bootstrap_deps {
            return Err(ModelsError::DeserializeError(
                "too many dependencies to deserialize".to_string(),
            ));
        }
        cursor += delta;
        let mut dependencies: Vec<BlockId> = Vec::with_capacity(dependencies_count as usize);
        for _ in 0..(dependencies_count as usize) {
            let dep = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += HASH_SIZE_BYTES;
            dependencies.push(dep);
        }

        Ok((
            ExportActiveBlock {
                is_final,
                block,
                parents,
                children,
                dependencies,
            },
            cursor,
        ))
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
        unsatisfied_dependencies: HashSet<BlockId>, // includes self if it's only a header
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
    pub children: Vec<HashSet<BlockId>>,
    /// Active or final
    pub status: Status,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Status {
    Active,
    Final,
}

#[derive(Debug, Default, Clone)]
pub struct ExportDiscardedBlocks {
    pub map: HashMap<BlockId, (DiscardReason, BlockHeader)>,
}

#[derive(Debug, Clone)]
pub struct BlockGraphExport {
    /// Genesis blocks.
    pub genesis_blocks: Vec<BlockId>,
    /// Map of active blocks, were blocks are in their exported version.
    pub active_blocks: HashMap<BlockId, ExportCompiledBlock>,
    /// Finite cache of discarded blocks, in exported version.
    pub discarded_blocks: ExportDiscardedBlocks,
    /// Best parents hashe in each thread.
    pub best_parents: Vec<BlockId>,
    /// Latest final period and block hash in each thread.
    pub latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// Head of the incompatibility graph.
    pub gi_head: HashMap<BlockId, HashSet<BlockId>>,
    /// List of maximal cliques of compatible blocks.
    pub max_cliques: Vec<HashSet<BlockId>>,
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
                                        .collect::<HashSet<BlockId>>()
                                })
                                .collect(),
                            status: if block.is_final {
                                Status::Final
                            } else {
                                Status::Active
                            },
                        },
                    );
                }
                _ => continue,
            }
        }

        export
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootsrapableGraph {
    /// Map of active blocks, were blocks are in their exported version.
    pub active_blocks: Vec<(BlockId, ExportActiveBlock)>,
    /// Best parents hashe in each thread.
    pub best_parents: Vec<BlockId>,
    /// Latest final period and block hash in each thread.
    pub latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// Head of the incompatibility graph.
    pub gi_head: Vec<(BlockId, Vec<BlockId>)>,
    /// List of maximal cliques of compatible blocks.
    pub max_cliques: Vec<Vec<BlockId>>,
}

impl<'a> From<&'a BlockGraph> for BootsrapableGraph {
    fn from(block_graph: &'a BlockGraph) -> Self {
        let mut active_blocks = HashMap::new();
        for (hash, status) in block_graph.block_statuses.iter() {
            match status {
                BlockStatus::Active(block) => {
                    active_blocks.insert(*hash, block.clone());
                }
                _ => continue,
            }
        }

        BootsrapableGraph {
            active_blocks: active_blocks
                .into_iter()
                .map(|(hash, block)| (hash, block.into()))
                .collect(),
            best_parents: block_graph.best_parents.clone(),
            latest_final_blocks_periods: block_graph.latest_final_blocks_periods.clone(),
            gi_head: block_graph
                .gi_head
                .clone()
                .into_iter()
                .map(|(hash, incomp)| (hash, incomp.into_iter().collect()))
                .collect(),
            max_cliques: block_graph
                .max_cliques
                .clone()
                .into_iter()
                .map(|clique| clique.into_iter().collect())
                .collect(),
        }
    }
}

impl SerializeCompact for BootsrapableGraph {
    fn to_bytes_compact(
        &self,
        context: &SerializationContext,
    ) -> Result<Vec<u8>, models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        //active_blocks
        let blocks_count: u32 = self.active_blocks.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many active blocks in BootsrapableGraph: {:?}",
                err
            ))
        })?;
        if blocks_count > context.max_bootstrap_blocks {
            return Err(ModelsError::SerializeError(format!("too many blocks in active_blocks for serialization context in BootstrapableGraph: {:?}", blocks_count)));
        }
        res.extend(u32::from(blocks_count).to_varint_bytes());
        for (hash, block) in self.active_blocks.iter() {
            res.extend(&hash.to_bytes());
            res.extend(block.to_bytes_compact(&context)?);
        }

        //best_parents
        for parent_h in self.best_parents.iter() {
            res.extend(&parent_h.to_bytes());
        }

        //latest_final_blocks_periods
        for (hash, period) in self.latest_final_blocks_periods.iter() {
            res.extend(&hash.to_bytes());
            res.extend(period.to_varint_bytes());
        }

        //gi_head
        let gi_head_count: u32 = self.gi_head.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many gi_head in BootsrapableGraph: {:?}", err))
        })?;
        res.extend(u32::from(gi_head_count).to_varint_bytes());
        for (gihash, set) in self.gi_head.iter() {
            res.extend(&gihash.to_bytes());
            let set_count: u32 = set.len().try_into().map_err(|err| {
                ModelsError::SerializeError(format!(
                    "too many entry in gi_head set in BootsrapableGraph: {:?}",
                    err
                ))
            })?;
            res.extend(u32::from(set_count).to_varint_bytes());
            for hash in set {
                res.extend(&hash.to_bytes());
            }
        }

        //max_cliques
        let max_cliques_count: u32 = self.max_cliques.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many max_cliques in BootsrapableGraph: {:?}",
                err
            ))
        })?;
        if max_cliques_count > context.max_bootstrap_cliques {
            return Err(ModelsError::SerializeError(format!("too many blocks in max_cliques for serialization context in BootstrapableGraph: {:?}", max_cliques_count)));
        }
        res.extend(u32::from(max_cliques_count).to_varint_bytes());
        for set in self.max_cliques.iter() {
            let set_count: u32 = set.len().try_into().map_err(|err| {
                ModelsError::SerializeError(format!(
                    "too many entry in max_cliques set in BootsrapableGraph: {:?}",
                    err
                ))
            })?;
            res.extend(u32::from(set_count).to_varint_bytes());
            for hash in set {
                res.extend(&hash.to_bytes());
            }
        }

        Ok(res)
    }
}

impl DeserializeCompact for BootsrapableGraph {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), models::ModelsError> {
        let mut cursor = 0usize;

        //active_blocks
        let (active_blocks_count, delta) = u32::from_varint_bytes(buffer)?;
        if active_blocks_count > context.max_bootstrap_blocks {
            return Err(ModelsError::DeserializeError(format!("too many blocks in active_blocks for deserialization context in BootstrapableGraph: {:?}", active_blocks_count)));
        }
        cursor += delta;
        let mut active_blocks: Vec<(BlockId, ExportActiveBlock)> =
            Vec::with_capacity(active_blocks_count as usize);
        for _ in 0..(active_blocks_count as usize) {
            let hash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += HASH_SIZE_BYTES;
            let (block, delta) =
                ExportActiveBlock::from_bytes_compact(&buffer[cursor..], &context)?;
            cursor += delta;
            active_blocks.push((hash, block));
        }

        //best_parents
        let mut best_parents: Vec<BlockId> = Vec::with_capacity(context.parent_count as usize);
        for _ in 0..context.parent_count {
            let parent_h = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += HASH_SIZE_BYTES;
            best_parents.push(parent_h);
        }

        //latest_final_blocks_periods
        let mut latest_final_blocks_periods: Vec<(BlockId, u64)> =
            Vec::with_capacity(context.parent_count as usize);
        for _ in 0..context.parent_count {
            let hash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += HASH_SIZE_BYTES;
            let (period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            latest_final_blocks_periods.push((hash, period));
        }

        //gi_head
        let (gi_head_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        if gi_head_count > context.max_bootstrap_blocks {
            return Err(ModelsError::DeserializeError(format!("too many blocks in gi_head for deserialization context in BootstrapableGraph: {:?}", gi_head_count)));
        }
        cursor += delta;
        let mut gi_head: Vec<(BlockId, Vec<BlockId>)> = Vec::with_capacity(gi_head_count as usize);
        for _ in 0..(gi_head_count as usize) {
            let gihash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += HASH_SIZE_BYTES;
            let (set_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
            if set_count > context.max_bootstrap_blocks {
                return Err(ModelsError::DeserializeError(format!("too many blocks in a set in gi_head for deserialization context in BootstrapableGraph: {:?}", set_count)));
            }
            cursor += delta;
            let mut set: Vec<BlockId> = Vec::with_capacity(set_count as usize);
            for _ in 0..(set_count as usize) {
                let hash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += HASH_SIZE_BYTES;
                set.push(hash);
            }
            gi_head.push((gihash, set));
        }

        //max_cliques: Vec<HashSet<BlockId>>
        let (max_cliques_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        if max_cliques_count > context.max_bootstrap_cliques {
            return Err(ModelsError::DeserializeError(format!("too many blocks in max_clilques for deserialization context in BootstrapableGraph: {:?}", max_cliques_count)));
        }
        cursor += delta;
        let mut max_cliques: Vec<Vec<BlockId>> = Vec::with_capacity(max_cliques_count as usize);
        for _ in 0..(max_cliques_count as usize) {
            let (set_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
            if set_count > context.max_bootstrap_blocks {
                return Err(ModelsError::DeserializeError(format!("too many blocks in a clique for deserialization context in BootstrapableGraph: {:?}", set_count)));
            }
            cursor += delta;
            let mut set: Vec<BlockId> = Vec::with_capacity(set_count as usize);
            for _ in 0..(set_count as usize) {
                let hash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += HASH_SIZE_BYTES;
                set.push(hash);
            }
            max_cliques.push(set);
        }

        Ok((
            BootsrapableGraph {
                active_blocks,
                best_parents,
                latest_final_blocks_periods,
                gi_head,
                max_cliques,
            },
            cursor,
        ))
    }
}

pub struct BlockGraph {
    cfg: ConsensusConfig,
    serialization_context: SerializationContext,
    genesis_hashes: Vec<BlockId>,
    sequence_counter: u64,
    block_statuses: HashMap<BlockId, BlockStatus>,
    latest_final_blocks_periods: Vec<(BlockId, u64)>,
    best_parents: Vec<BlockId>,
    gi_head: HashMap<BlockId, HashSet<BlockId>>,
    max_cliques: Vec<HashSet<BlockId>>,
    to_propagate: HashMap<BlockId, Block>,
    attack_attempts: Vec<BlockId>,
}

#[derive(Debug)]
enum CheckOutcome {
    Proceed {
        parents_hash_period: Vec<(BlockId, u64)>,
        dependencies: HashSet<BlockId>,
        incompatibilities: HashSet<BlockId>,
        inherited_incompatibilities_count: usize,
    },
    Discard(DiscardReason),
    WaitForSlot,
    WaitForDependencies(HashSet<BlockId>),
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
) -> Result<(BlockId, Block), ConsensusError> {
    let private_key = cfg.genesis_key;
    let public_key = derive_public_key(&private_key);
    let (header_hash, header) = BlockHeader::new_signed(
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
        init: Option<BootsrapableGraph>,
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
                    descendants: HashSet::new(),
                    is_final: true,
                }),
            );
        }

        massa_trace!("consensus.block_graph.new", {});
        if let Some(boot_graph) = init {
            let mut res_graph = BlockGraph {
                cfg,
                serialization_context,
                sequence_counter: 0,
                genesis_hashes: block_hashes.clone(),
                block_statuses: boot_graph
                    .active_blocks
                    .iter()
                    .map(|(hash, block)| (*hash, BlockStatus::Active(block.clone().into())))
                    .collect(),
                latest_final_blocks_periods: boot_graph.latest_final_blocks_periods,
                best_parents: boot_graph.best_parents,
                gi_head: boot_graph
                    .gi_head
                    .into_iter()
                    .map(|(h, v)| (h, v.into_iter().collect()))
                    .collect(),
                max_cliques: boot_graph
                    .max_cliques
                    .into_iter()
                    .map(|v| v.into_iter().collect())
                    .collect(),
                to_propagate: Default::default(),
                attack_attempts: Default::default(),
            };
            // compute block descendants
            let active_blocks_map: HashMap<BlockId, Vec<BlockId>> = res_graph
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
                let mut visited: HashSet<BlockId> = HashSet::new();
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
    }

    /// Returns hash and resulting discarded blocks
    ///
    /// # Arguments
    /// * val : dummy value used to generate dummy hash
    /// * slot : generated block is in slot slot.
    pub fn create_block(
        &self,
        val: String,
        slot: Slot,
    ) -> Result<(BlockId, Block), ConsensusError> {
        let (public_key, private_key) = self
            .cfg
            .nodes
            .get(self.cfg.current_node_index as usize)
            .and_then(|(public_key, private_key)| Some((public_key.clone(), private_key.clone())))
            .ok_or(ConsensusError::KeyError)?;

        let example_hash = Hash::hash(&val.as_bytes());

        let (hash, header) = BlockHeader::new_signed(
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
        let res = (
            hash,
            Block {
                header,
                operations: Vec::new(),
            },
        );
        massa_trace!("consensus.block_graph.create_block", {"hash": res.0, "block": res.1});
        Ok(res)
    }

    /// Gets lastest final blocks (hash, period) for each thread.
    pub fn get_latest_final_blocks_periods(&self) -> &Vec<(BlockId, u64)> {
        &self.latest_final_blocks_periods
    }

    /// Gets whole block corresponding to given hash, if it is active.
    ///
    /// # Argument
    /// * hash : header's hash of the given block.
    pub fn get_active_block(&self, hash: BlockId) -> Option<&Block> {
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
        let to_process: BTreeSet<(Slot, BlockId)> = self
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

        massa_trace!("consensus.block_graph.slot_tick", {});
        // process those elements
        self.rec_process(to_process, selector, current_slot)?;

        Ok(())
    }

    pub fn incoming_header(
        &mut self,
        hash: BlockId,
        header: BlockHeader,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<(), ConsensusError> {
        // ignore genesis blocks
        if self.genesis_hashes.contains(&hash) {
            return Ok(());
        }

        massa_trace!("consensus.block_graph.incoming_header", {"hash": hash, "header": header});
        let mut to_ack: BTreeSet<(Slot, BlockId)> = BTreeSet::new();
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
        block_id: BlockId,
        block: Block,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<(), ConsensusError> {
        // ignore genesis blocks
        if self.genesis_hashes.contains(&block_id) {
            return Ok(());
        }

        massa_trace!("consensus.block_graph.incoming_block", {"block_id": block_id, "block": block});
        let mut to_ack: BTreeSet<(Slot, BlockId)> = BTreeSet::new();
        match self.block_statuses.entry(block_id) {
            // if absent => add as Incoming, call rec_ack on it
            hash_map::Entry::Vacant(vac) => {
                to_ack.insert((block.header.content.slot, block_id));
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
                    if unsatisfied_dependencies.remove(&block_id) {
                        // a dependency was satisfied: process
                        to_ack.insert((block.header.content.slot, block_id));
                    }
                    *header_or_block = HeaderOrBlock::Block(block);
                    // promote in dependencies
                    self.promote_dep_tree(block_id)?;
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
        mut to_ack: BTreeSet<(Slot, BlockId)>,
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
        hash: BlockId,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<BTreeSet<(Slot, BlockId)>, ConsensusError> {
        // list items to reprocess
        let mut reprocess = BTreeSet::new();

        massa_trace!("consensus.block_graph.process", { "hash": hash });
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
            Some(BlockStatus::Discarded { .. }) => {
                massa_trace!("consensus.block_graph.process.discarded", { "hash": hash });
                return Ok(BTreeSet::new());
            }

            // already active: do nothing
            Some(BlockStatus::Active(_)) => {
                massa_trace!("consensus.block_graph.process.active", { "hash": hash });
                return Ok(BTreeSet::new());
            }

            // incoming header
            Some(BlockStatus::Incoming(HeaderOrBlock::Header(_))) => {
                massa_trace!("consensus.block_graph.process.incomming_header", {
                    "hash": hash
                });
                // remove header
                let header = if let Some(BlockStatus::Incoming(HeaderOrBlock::Header(header))) =
                    self.block_statuses.remove(&hash)
                {
                    header
                } else {
                    return Err(ConsensusError::ContainerInconsistency(format!(
                        "inconsistency inside block statuses removing incomming header {:?}",
                        hash
                    )));
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

                        massa_trace!(
                            "consensus.block_graph.process.incomming_header.waiting_for_self",
                            { "hash": hash }
                        );
                        return Ok(BTreeSet::new());
                    }
                    CheckOutcome::WaitForDependencies(mut dependencies) => {
                        // set as waiting dependencies
                        dependencies.insert(hash); // add self as unsatisfied
                        massa_trace!("consensus.block_graph.process.incomming_header.waiting_for_dependencies", {"hash": hash, "dependencies": dependencies});

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

                        massa_trace!(
                            "consensus.block_graph.process.incomming_header.waiting_for_slot",
                            { "hash": hash }
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

                        massa_trace!("consensus.block_graph.process.incomming_header.discarded", {"hash": hash, "reason": reason});
                        return Ok(BTreeSet::new());
                    }
                }
            }

            // incoming block
            Some(BlockStatus::Incoming(HeaderOrBlock::Block(_))) => {
                massa_trace!("consensus.block_graph.process.incomming_block", {
                    "hash": hash
                });
                let block = if let Some(BlockStatus::Incoming(HeaderOrBlock::Block(block))) =
                    self.block_statuses.remove(&hash)
                {
                    block
                } else {
                    return Err(ConsensusError::ContainerInconsistency(format!(
                        "inconsistency inside block statuses removing incomming block {:?}",
                        hash
                    )));
                };
                match self.check_block(hash, &block, selector, current_slot)? {
                    CheckOutcome::Proceed {
                        parents_hash_period,
                        dependencies,
                        incompatibilities,
                        inherited_incompatibilities_count,
                    } => {
                        // block is valid: remove it from Incoming and return it

                        massa_trace!("consensus.block_graph.process.incomming_block.valid", {
                            "hash": hash
                        });
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
                        massa_trace!("consensus.block_graph.process.incomming_block.waiting_for_dependencies", {"hash": hash});
                        return Ok(BTreeSet::new());
                    }
                    CheckOutcome::WaitForSlot => {
                        // set as waiting for slot
                        self.block_statuses.insert(
                            hash,
                            BlockStatus::WaitingForSlot(HeaderOrBlock::Block(block)),
                        );

                        massa_trace!(
                            "consensus.block_graph.process.incomming_block.waiting_for_slot",
                            { "hash": hash }
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

                        massa_trace!("consensus.block_graph.process.incomming_block.discarded", {"hash": hash, "reason": reason});
                        return Ok(BTreeSet::new());
                    }
                }
            }

            Some(BlockStatus::WaitingForSlot(header_or_block)) => {
                massa_trace!("consensus.block_graph.process.waiting_for_slot", {
                    "hash": hash
                });
                let slot = header_or_block.get_slot();
                if Some(slot) > current_slot {
                    massa_trace!(
                        "consensus.block_graph.process.waiting_for_slot.in_the_future",
                        { "hash": hash }
                    );
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
                    massa_trace!(
                        "consensus.block_graph.process.waiting_for_slot.reprocess",
                        { "hash": hash }
                    );
                    return Ok(reprocess);
                } else {
                    return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses removing waiting for slot block or header {:?}", hash)));
                };
            }

            Some(BlockStatus::WaitingForDependencies {
                unsatisfied_dependencies,
                ..
            }) => {
                massa_trace!("consensus.block_graph.process.waiting_for_dependencies", {
                    "hash": hash
                });
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
                    massa_trace!(
                        "consensus.block_graph.process.waiting_for_dependencies.reprocess",
                        { "hash": hash }
                    );
                    return Ok(reprocess);
                } else {
                    return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses removing waiting for slot header or block {:?}", hash)));
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
            massa_trace!("consensus.block_graph.process.is_active", { "hash": hash });
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
    fn maybe_note_attack_attempt(&mut self, reason: &DiscardReason, hash: &BlockId) {
        massa_trace!("consensus.block_graph.maybe_note_attack_attempt", {"hash": hash, "reason": reason});
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
        block_statuses: &HashMap<BlockId, BlockStatus>,
        hash: BlockId,
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
        hash: BlockId,
    ) -> Result<HashSet<BlockId>, ConsensusError> {
        let mut to_visit = vec![hash];
        let mut result: HashSet<BlockId> = HashSet::new();
        while let Some(visit_h) = to_visit.pop() {
            if !result.insert(visit_h) {
                continue; // already visited
            }
            BlockGraph::get_full_active_block(&self.block_statuses, visit_h)
                .ok_or(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses iterating through descendants of {:?} - missing {:?}", hash, visit_h)))?
                .children
                .iter()
                .for_each(|thread_children| to_visit.extend(thread_children.keys()));
        }
        Ok(result)
    }

    fn check_header(
        &self,
        hash: BlockId,
        header: &BlockHeader,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<CheckOutcome, ConsensusError> {
        massa_trace!("consensus.block_graph.check_header", { "hash": hash });
        let mut parents: Vec<(BlockId, u64)> = Vec::with_capacity(self.cfg.thread_count as usize);
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
        let parent_set: HashSet<BlockId> = header.content.parents.iter().copied().collect();
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
                let parent = self.get_active_block(parent_h).ok_or(
                    ConsensusError::ContainerInconsistency(format!(
                        "inconsistency inside block statuses searching parent {:?} of block {:?}",
                        parent_h, hash
                    )),
                )?;
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
        .ok_or(ConsensusError::ContainerInconsistency(format!(
            "inconsistency inside block statuses searching parent {:?} in own thread of block {:?}",
            parents[header.content.slot.thread as usize].0, hash
        )))?;

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
                    .ok_or(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses searching {:?} while checking gradpa incompatibility of block {:?}",cur_h,  hash)))?;

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
                .ok_or(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses searching {:?} check if the parent in tauB has a strictly lower period number than B's parent in tauB while checking gradpa incompatibility of block {:?}",cur_b.block.header.content.parents[header.content.slot.thread as usize],  hash)))?
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
        massa_trace!("consensus.block_graph.check_header.ok", { "hash": hash });

        Ok(CheckOutcome::Proceed {
            parents_hash_period: parents,
            dependencies: deps,
            incompatibilities: incomp,
            inherited_incompatibilities_count: inherited_incomp_count,
        })
    }

    fn check_block(
        &self,
        hash: BlockId,
        block: &Block,
        selector: &mut RandomSelector,
        current_slot: Option<Slot>,
    ) -> Result<CheckOutcome, ConsensusError> {
        massa_trace!("consensus.block_graph.check_block", { "hash": hash });
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

        massa_trace!("consensus.block_graph.check_block.ok", { "hash": hash });

        Ok(CheckOutcome::Proceed {
            parents_hash_period: parents,
            dependencies: deps,
            incompatibilities: incomp,
            inherited_incompatibilities_count: inherited_incomp_count,
        })
    }

    /// Computes max cliques of compatible blocks
    fn compute_max_cliques(&self) -> Vec<HashSet<BlockId>> {
        let mut max_cliques: Vec<HashSet<BlockId>> = Vec::new();

        // algorithm adapted from IK_GPX as summarized in:
        //   Cazals et al., "A note on the problem of reporting maximal cliques"
        //   Theoretical Computer Science, 2008
        //   https://doi.org/10.1016/j.tcs.2008.05.010

        // stack: r, p, x
        let mut stack: Vec<(HashSet<BlockId>, HashSet<BlockId>, HashSet<BlockId>)> = vec![(
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
            let u_set: HashSet<BlockId> =
                &p & &(&self.gi_head[&u_p] | &vec![u_p].into_iter().collect());
            for u_i in u_set.into_iter() {
                p.remove(&u_i);
                let u_i_set: HashSet<BlockId> = vec![u_i].into_iter().collect();
                let comp_n_u_i: HashSet<BlockId> = &self.gi_head[&u_i] | &u_i_set;
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
        hash: BlockId,
        parents_hash_period: Vec<(BlockId, u64)>,
        block: Block,
        deps: HashSet<BlockId>,
        incomp: HashSet<BlockId>,
        inherited_incomp_count: usize,
    ) -> Result<(), ConsensusError> {
        // add block to status structure
        massa_trace!("consensus.block_graph.add_block_to_graph", { "hash": hash });
        self.block_statuses.insert(
            hash,
            BlockStatus::Active(ActiveBlock {
                parents: parents_hash_period.clone(),
                dependencies: deps,
                descendants: HashSet::new(),
                block: block.clone(),
                children: vec![HashMap::new(); self.cfg.thread_count as usize],
                is_final: false,
            }),
        );

        // add as child to parents
        for (parent_h, _parent_period) in parents_hash_period.iter() {
            if let Some(BlockStatus::Active(a_parent)) = self.block_statuses.get_mut(parent_h) {
                a_parent.children[block.header.content.slot.thread as usize]
                    .insert(hash, block.header.content.slot.period);
            } else {
                return Err(ConsensusError::ContainerInconsistency(format!(
                    "inconsistency inside block statuses adding child {:?} of block {:?}",
                    hash, parent_h
                )));
            }
        }

        // add as descendant to ancestors. Note: descendants are never removed.
        {
            let mut ancestors: VecDeque<BlockId> =
                parents_hash_period.iter().map(|(h, _)| *h).collect();
            let mut visited: HashSet<BlockId> = HashSet::new();
            while let Some(ancestor_h) = ancestors.pop_back() {
                if !visited.insert(ancestor_h) {
                    continue;
                }
                if let Some(BlockStatus::Active(ab)) = self.block_statuses.get_mut(&ancestor_h) {
                    ab.descendants.insert(hash);
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
                .ok_or(ConsensusError::MissingBlock)?
                .insert(hash);
        }
        self.gi_head.insert(hash, incomp.clone());

        // max cliques update
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.max_cliques_update",
            {}
        );
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
            massa_trace!(
                "consensus.block_graph.add_block_to_graph.clique_full_computing",
                { "hash": hash }
            );
            let before = self.max_cliques.len();
            self.max_cliques = self.compute_max_cliques();
            let after = self.max_cliques.len();
            if before != after {
                massa_trace!(
                    "consensus.block_graph.add_block_to_graph.clique_full_computing more than one clique",
                    { "cliques": self.max_cliques, "gi_head": self.gi_head }
                );
                //gi_head
                warn!(
                    "clique number went from {:?} to {:?} after adding {:?}",
                    before, after, hash
                );
            }
        }

        // compute clique fitnesses and find blockclique
        massa_trace!("consensus.block_graph.add_block_to_graph.compute_clique_fitnesses_and_find_blockclique", {});
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
                            .ok_or(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses computing fitness while adding {:?} - missing {:?}", hash, block_h)))?
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
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.update_best_parents",
            {}
        );
        {
            let blockclique = &self.max_cliques[blockclique_i];
            let mut parents_updated = 0u8;
            for block_h in blockclique.iter() {
                let block_a = BlockGraph::get_full_active_block(&self.block_statuses, *block_h)
                    .ok_or(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses updating best parents while adding {:?} - missing {:?}", hash, block_h)))?;
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
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.list_stale_blocks",
            {}
        );
        let stale_blocks = {
            let fitnesss_threshold = clique_fitnesses[blockclique_i]
                .0
                .saturating_sub(self.cfg.delta_f0);
            // iterate from largest to smallest to minimize reallocations
            let mut indices: Vec<usize> = (0..self.max_cliques.len()).collect();
            indices.sort_unstable_by_key(|&i| std::cmp::Reverse(self.max_cliques[i].len()));
            let mut high_set: HashSet<BlockId> = HashSet::new();
            let mut low_set: HashSet<BlockId> = HashSet::new();
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
        massa_trace!(
            "consensus.block_graph.add_block_to_graph.mark_stale_blocks",
            {}
        );
        for stale_block_hash in stale_blocks.into_iter() {
            if let Some(BlockStatus::Active(active_block)) =
                self.block_statuses.remove(&stale_block_hash)
            {
                if active_block.is_final {
                    return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses removing stale blocks adding {:?} - block {:?} was already final", hash, stale_block_hash)));
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
                        children[active_block.block.header.content.slot.thread as usize]
                            .remove(&stale_block_hash);
                    }
                }

                massa_trace!("consensus.block_graph.add_block_to_graph.stale", {
                    "hash": stale_block_hash
                });
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
                return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses removing stale blocks adding {:?} - block {:?} is missing", hash, stale_block_hash)));
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
            indices.sort_unstable_by_key(|&i| self.max_cliques[i].len());
            let mut final_candidates = self.max_cliques[indices[0]].clone();
            for i in 1..indices.len() {
                final_candidates.retain(|v| self.max_cliques[i].contains(v));
                if final_candidates.is_empty() {
                    break;
                }
            }

            // restrict search to cliques with high enough fitness, sort cliques by fitness (highest to lowest)
            massa_trace!(
                "consensus.block_graph.add_block_to_graph.list_final_blocks.restrict",
                {}
            );
            indices.retain(|&i| clique_fitnesses[i].0 > self.cfg.delta_f0);
            indices.sort_unstable_by_key(|&i| std::cmp::Reverse(clique_fitnesses[i].0));

            let mut final_blocks: HashSet<BlockId> = HashSet::new();
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
                            .ok_or(ConsensusError::MissingBlock)?
                            .descendants
                            .intersection(clique)
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
                massa_trace!("consensus.block_graph.add_block_to_graph.final", {
                    "hash": final_block_hash
                });
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
                return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses updating final blocks adding {:?} - block {:?} is missing", hash, final_block_hash)));
            }
        }

        massa_trace!("consensus.block_graph.add_block_to_graph.end", {});
        Ok(())
    }

    // prune active blocks and return final blocks, return discarded final blocks
    fn prune_active(&mut self) -> Result<HashMap<BlockId, Block>, ConsensusError> {
        // list all active blocks
        let active_blocks: HashSet<BlockId> = self
            .block_statuses
            .iter()
            .filter_map(|(h, bs)| match bs {
                BlockStatus::Active(_) => Some(*h),
                _ => None,
            })
            .collect();

        let mut retain_active: HashSet<BlockId> = HashSet::new();

        let latest_final_blocks: Vec<BlockId> = self
            .latest_final_blocks_periods
            .iter()
            .map(|(hash, _)| hash.clone())
            .collect();

        // retain all non-final active blocks,
        // the current "best parents",
        // and the dependencies for both.
        for (hash, block_status) in self.block_statuses.iter() {
            if let BlockStatus::Active(ActiveBlock {
                is_final,
                dependencies,
                ..
            }) = block_status
            {
                if !*is_final
                    || self.best_parents.contains(hash)
                    || latest_final_blocks.contains(hash)
                {
                    retain_active.extend(dependencies);
                    retain_active.insert(*hash);
                }
            }
        }

        // retain best parents
        retain_active.extend(&self.best_parents);

        // retain last final blocks
        retain_active.extend(
            self.latest_final_blocks_periods
                .iter()
                .map(|(h, _)| h.clone()),
        );

        // grow with parents & fill thread holes twice
        for _ in 0..2 {
            // retain the parents of the selected blocks
            let retain_clone = retain_active.clone();

            for retain_h in retain_clone.into_iter() {
                retain_active.extend(
                    self.get_active_block(retain_h)
                        .ok_or(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses pruning and retaining the parents of the selected blocks - {:?} is missing", retain_h)))?
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
                    .ok_or(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses pruning and finding earliest kept slots in each thread - {:?} is missing", retain_h)))?
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
        let mut discarded_finals: HashMap<BlockId, Block> = HashMap::new();
        for discard_active_h in active_blocks.difference(&retain_active) {
            let discarded_active = if let Some(BlockStatus::Active(discarded_active)) =
                self.block_statuses.remove(discard_active_h)
            {
                discarded_active
            } else {
                return Err(ConsensusError::ContainerInconsistency(format!("inconsistency inside block statuses pruning and removing unused final active blocks - {:?} is missing", discard_active_h)));
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

            massa_trace!("consensus.block_graph.prune_active", {"hash": discard_active_h, "reason": DiscardReason::Final});
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

    fn promote_dep_tree(&mut self, hash: BlockId) -> Result<(), ConsensusError> {
        let mut to_explore = vec![hash];
        let mut to_promote: HashMap<BlockId, (Slot, u64)> = HashMap::new();
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

    fn prune_waiting_for_dependencies(&mut self) -> Result<(), ConsensusError> {
        let mut to_discard: HashMap<BlockId, Option<DiscardReason>> = HashMap::new();
        let mut to_keep: HashMap<BlockId, (u64, Slot)> = HashMap::new();

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
                massa_trace!("consensus.block_graph.prune_waiting_for_dependencies", {"hash": hash, "reason": reason_opt});
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
        let mut slot_waiting: Vec<(Slot, BlockId)> = self
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
        let retained: HashSet<BlockId> = slot_waiting
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
        let mut discard_hashes: Vec<(u64, BlockId)> = self
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
    pub fn prune(&mut self) -> Result<HashMap<BlockId, Block>, ConsensusError> {
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
            warn!(
                "clique number went from {:?} to {:?} after pruning",
                before, after
            );
        }

        Ok(discarded_finals)
    }

    // get the current block wishlist
    pub fn get_block_wishlist(&self) -> Result<HashSet<BlockId>, ConsensusError> {
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
    pub fn get_blocks_to_propagate(&mut self) -> HashMap<BlockId, Block> {
        mem::take(&mut self.to_propagate)
    }

    // Get the hashes of objects that were attack attempts.
    // Must be called by the consensus worker within `block_db_changed`.
    pub fn get_attack_attempts(&mut self) -> Vec<BlockId> {
        mem::take(&mut self.attack_attempts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::UTime;

    pub fn get_export_active_test_block() -> ExportActiveBlock {
        let block = Block {
            header: BlockHeader {
                content: BlockHeaderContent{
                    creator: crypto::signature::PublicKey::from_bs58_check("4vYrPNzUM8PKg2rYPW3ZnXPzy67j9fn5WsGCbnwAnk2Lf7jNHb").unwrap(),
                    operation_merkle_root: Hash::hash("operation_merkle_root".as_bytes()),
                    out_ledger_hash: Hash::hash("out_ledger_hash".as_bytes()),
                    parents: vec![
                        BlockId::for_tests("parent1").unwrap(),
                        BlockId::for_tests("parent2").unwrap(),
                    ],
                    slot: Slot::new(1, 0),
                },
                signature: crypto::signature::Signature::from_bs58_check(
                    "5f4E3opXPWc3A1gvRVV7DJufvabDfaLkT1GMterpJXqRZ5B7bxPe5LoNzGDQp9LkphQuChBN1R5yEvVJqanbjx7mgLEae"
                ).unwrap()
            },
            operations: vec![]
        };

        ExportActiveBlock {
            parents: vec![
                (BlockId::for_tests("parent11").unwrap(), 23),
                (BlockId::for_tests("parent12").unwrap(), 24),
            ],
            dependencies: vec![
                BlockId::for_tests("dep11").unwrap(),
                BlockId::for_tests("dep12").unwrap(),
                BlockId::for_tests("dep13").unwrap(),
            ]
            .into_iter()
            .collect(),
            block,
            children: vec![vec![
                (BlockId::for_tests("child11").unwrap(), 31),
                (BlockId::for_tests("child11").unwrap(), 31),
            ]
            .into_iter()
            .collect()],
            is_final: true,
        }
    }

    #[test]
    fn test_bootsrapable_graph_serialize_compact() {
        //test with 2 thread
        let serialization_context = SerializationContext {
            max_block_size: 1024 * 1024,
            max_block_operations: 1024,
            parent_count: 2,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_ask_blocks_per_message: 10,
            max_operations_per_message: 1024,
            max_bootstrap_message_size: 100000000,
        };

        let active_block = get_export_active_test_block();

        let bytes = active_block
            .block
            .to_bytes_compact(&serialization_context)
            .unwrap();
        let new_block = Block::from_bytes_compact(&bytes, &serialization_context).unwrap();

        println!("{:?}", new_block);

        let graph = BootsrapableGraph {
            /// Map of active blocks, were blocks are in their exported version.
            active_blocks: vec![
                (
                    BlockId::for_tests("active11").unwrap(),
                    active_block.clone(),
                ),
                (
                    BlockId::for_tests("active12").unwrap(),
                    active_block.clone(),
                ),
                (
                    BlockId::for_tests("active13").unwrap(),
                    active_block.clone(),
                ),
            ]
            .into_iter()
            .collect(),
            /// Best parents hashe in each thread.
            best_parents: vec![
                BlockId::for_tests("parent11").unwrap(),
                BlockId::for_tests("parent12").unwrap(),
            ],
            /// Latest final period and block hash in each thread.
            latest_final_blocks_periods: vec![
                (BlockId::for_tests("lfinal11").unwrap(), 23),
                (BlockId::for_tests("lfinal12").unwrap(), 24),
            ],
            /// Head of the incompatibility graph.
            gi_head: vec![
                (
                    BlockId::for_tests("gi_head11").unwrap(),
                    vec![
                        BlockId::for_tests("set11").unwrap(),
                        BlockId::for_tests("set12").unwrap(),
                    ],
                ),
                (
                    BlockId::for_tests("gi_head12").unwrap(),
                    vec![
                        BlockId::for_tests("set21").unwrap(),
                        BlockId::for_tests("set22").unwrap(),
                    ],
                ),
                (
                    BlockId::for_tests("gi_head13").unwrap(),
                    vec![
                        BlockId::for_tests("set31").unwrap(),
                        BlockId::for_tests("set32").unwrap(),
                    ],
                ),
            ]
            .into_iter()
            .collect(),

            /// List of maximal cliques of compatible blocks.
            max_cliques: vec![vec![
                BlockId::for_tests("max_cliques11").unwrap(),
                BlockId::for_tests("max_cliques12").unwrap(),
            ]
            .into_iter()
            .collect()],
        };

        let bytes = graph.to_bytes_compact(&serialization_context).unwrap();
        let (new_graph, cursor) =
            BootsrapableGraph::from_bytes_compact(&bytes, &serialization_context).unwrap();

        assert_eq!(bytes.len(), cursor);
        assert_eq!(
            graph.active_blocks[0].1.block.header.signature,
            new_graph.active_blocks[0].1.block.header.signature
        );
        assert_eq!(graph.best_parents[0], new_graph.best_parents[0]);
        assert_eq!(graph.best_parents[1], new_graph.best_parents[1]);
        assert_eq!(
            graph.latest_final_blocks_periods[0],
            new_graph.latest_final_blocks_periods[0]
        );
        assert_eq!(
            graph.latest_final_blocks_periods[1],
            new_graph.latest_final_blocks_periods[1]
        );
    }

    #[test]
    fn test_clique_calculation() {
        let (cfg, serialization_context) = example_consensus_config();
        let mut block_graph = BlockGraph::new(cfg, serialization_context, None).unwrap();
        let hashes: Vec<BlockId> = vec![
            "VzCRpnoZVYY1yQZTXtVQbbxwzdu6hYtdCUZB5BXWSabsiXyfP",
            "JnWwNHRR1tUD7UJfnEFgDB4S4gfDTX2ezLadr7pcwuZnxTvn1",
            "xtvLedxC7CigAPytS5qh9nbTuYyLbQKCfbX8finiHsKMWH6SG",
            "2Qs9sSbc5sGpVv5GnTeDkTKdDpKhp4AgCVT4XFcMaf55msdvJN",
            "2VNc8pR4tNnZpEPudJr97iNHxXbHiubNDmuaSzrxaBVwKXxV6w",
            "2bsrYpfLdvVWAJkwXoJz1kn4LWshdJ6QjwTrA7suKg8AY3ecH1",
            "kfUeGj3ZgBprqFRiAQpE47dW5tcKTAueVaWXZquJW6SaPBd4G",
        ]
        .into_iter()
        .map(|h| BlockId::from_bs58_check(h).unwrap())
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

        let expected_sets: Vec<HashSet<BlockId>> = vec![
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
        let genesis_key = crypto::generate_random_private_key();
        let mut nodes = Vec::new();
        for _ in 0..2 {
            let private_key = crypto::generate_random_private_key();
            let public_key = crypto::derive_public_key(&private_key);
            nodes.push((public_key, private_key));
        }
        let thread_count: u8 = 2;
        let max_block_size = 1024 * 1024;
        let max_operations_per_block = 1024;
        (
            ConsensusConfig {
                genesis_timestamp: UTime::now(0).unwrap(),
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
                operation_validity_periods: 3,
            },
            SerializationContext {
                max_block_size,
                max_block_operations: max_operations_per_block,
                parent_count: thread_count,
                max_peer_list_length: 128,
                max_message_size: 3 * 1024 * 1024,
                max_bootstrap_blocks: 100,
                max_bootstrap_cliques: 100,
                max_bootstrap_deps: 100,
                max_bootstrap_children: 100,
                max_ask_blocks_per_message: 10,
                max_operations_per_message: 1024,
                max_bootstrap_message_size: 100000000,
            },
        )
    }
}
