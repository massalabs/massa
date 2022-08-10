use crate::error::{GraphError, GraphResult as Result};
use massa_models::{
    active_block::ActiveBlock,
    array_from_slice,
    constants::*,
    prehash::{BuildMap, Map, Set},
    u8_from_slice, with_serialization_context,
    wrapped::{WrappedDeserializer, WrappedSerializer},
    BlockDeserializer, BlockId, DeserializeCompact, DeserializeVarInt, ModelsError,
    SerializeCompact, SerializeVarInt, WrappedBlock,
};
use massa_serialization::{Deserializer, Serializer};
use massa_storage::Storage;
use serde::{Deserialize, Serialize};

/// Exportable version of `ActiveBlock`
/// Fields that can be easily recomputed were left out
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportActiveBlock {
    /// The block.
    pub block: WrappedBlock,
    /// The Id of the block.
    pub block_id: BlockId,
    /// one `(block id, period)` per thread ( if not genesis )
    pub parents: Vec<(BlockId, u64)>,
    /// one `HashMap<Block id, period>` per thread (blocks that need to be kept)
    /// Children reference that block as a parent
    pub children: Vec<Map<BlockId, u64>>,
    /// dependencies required for validity check
    pub dependencies: Set<BlockId>,
    /// for example has its fitness reached the given threshold
    pub is_final: bool,
}

impl TryFrom<ExportActiveBlock> for ActiveBlock {
    fn try_from(a_block: ExportActiveBlock) -> Result<ActiveBlock> {
        //TODO export full objects (block, ops, endorsements) loaded from storage

        let operation_set = a_block
            .block
            .content
            .operations
            .iter()
            .enumerate()
            .map(|(idx, op)| (op.id, (idx, op.content.expire_period)))
            .collect();

        let endorsement_ids = a_block
            .block
            .content
            .header
            .content
            .endorsements
            .iter()
            .map(|endo| (endo.id, endo.content.index))
            .collect();

        let addresses_to_operations = a_block.block.involved_addresses(&operation_set)?;
        let addresses_to_endorsements = a_block.block.addresses_to_endorsements()?;
        Ok(ActiveBlock {
            creator_address: a_block.block.creator_address,
            block_id: a_block.block_id,
            parents: a_block.parents.clone(),
            children: a_block.children.clone(),
            dependencies: a_block.dependencies.clone(),
            descendants: Default::default(), // will be computed once the full graph is available
            is_final: a_block.is_final,
            operation_set,
            endorsement_ids,
            addresses_to_operations,
            addresses_to_endorsements,
            slot: a_block.block.content.header.content.slot,
        })
    }
    type Error = GraphError;
}

impl ExportActiveBlock {
    /// try conversion from active block to export active block
    pub fn try_from_active_block(a_block: &ActiveBlock, storage: Storage) -> Result<Self> {
        let block = storage.retrieve_block(&a_block.block_id).ok_or_else(|| {
            GraphError::MissingBlock(format!(
                "missing block ExportActiveBlock::try_from_active_block: {}",
                a_block.block_id
            ))
        })?;
        let stored_block = block.read();
        Ok(ExportActiveBlock {
            block: stored_block.clone(),
            block_id: a_block.block_id,
            parents: a_block.parents.clone(),
            children: a_block.children.clone(),
            dependencies: a_block.dependencies.clone(),
            is_final: a_block.is_final,
        })
    }
}

impl SerializeCompact for ExportActiveBlock {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // is_final
        if self.is_final {
            res.push(1);
        } else {
            res.push(0);
        }

        // block
        WrappedSerializer::new().serialize(&self.block, &mut res)?;

        // parents (note: there should be none if slot period=0)
        if self.parents.is_empty() {
            res.push(0);
        } else {
            res.push(1);
        }
        for (hash, period) in self.parents.iter() {
            res.extend(hash.to_bytes());
            res.extend(period.to_varint_bytes());
        }

        // children
        let children_count: u32 = self.children.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many children in ActiveBlock: {}", err))
        })?;
        res.extend(children_count.to_varint_bytes());
        for map in self.children.iter() {
            let map_count: u32 = map.len().try_into().map_err(|err| {
                ModelsError::SerializeError(format!(
                    "too many entry in children map in ActiveBlock: {}",
                    err
                ))
            })?;
            res.extend(map_count.to_varint_bytes());
            for (hash, period) in map {
                res.extend(hash.to_bytes());
                res.extend(period.to_varint_bytes());
            }
        }

        // dependencies
        let dependencies_count: u32 = self.dependencies.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many dependencies in ActiveBlock: {}", err))
        })?;
        res.extend(dependencies_count.to_varint_bytes());
        for dep in self.dependencies.iter() {
            res.extend(dep.to_bytes());
        }

        Ok(res)
    }
}

impl DeserializeCompact for ExportActiveBlock {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;
        let (parent_count, max_bootstrap_children, max_bootstrap_deps) =
            with_serialization_context(|context| {
                (
                    context.thread_count,
                    context.max_bootstrap_children,
                    context.max_bootstrap_deps,
                )
            });

        // is_final
        let is_final_u8 = u8_from_slice(buffer)?;
        cursor += 1;
        let is_final = is_final_u8 != 0;

        // block
        let (rest, block): (&[u8], WrappedBlock) =
            WrappedDeserializer::new(BlockDeserializer::new()).deserialize(&buffer[cursor..])?;
        cursor += buffer[cursor..].len() - rest.len();

        // parents
        let has_parents = u8_from_slice(&buffer[cursor..])?;
        cursor += 1;
        let parents = if has_parents == 1 {
            let mut parents: Vec<(BlockId, u64)> = Vec::with_capacity(parent_count as usize);
            for _ in 0..parent_count {
                let parent_h = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?);
                cursor += BLOCK_ID_SIZE_BYTES;
                let (period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;

                parents.push((parent_h, period));
            }
            parents
        } else if has_parents == 0 {
            Vec::new()
        } else {
            return Err(ModelsError::SerializeError(
                "ActiveBlock from_bytes_compact bad has parents flags.".into(),
            ));
        };

        // children
        let (children_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        let parent_count_u32: u32 = parent_count.into();
        if children_count > parent_count_u32 {
            return Err(ModelsError::DeserializeError(
                "too many threads with children to deserialize".to_string(),
            ));
        }
        cursor += delta;
        let mut children: Vec<Map<BlockId, u64>> = Vec::with_capacity(children_count as usize);
        for _ in 0..(children_count as usize) {
            let (map_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
            if map_count > max_bootstrap_children {
                return Err(ModelsError::DeserializeError(
                    "too many children to deserialize".to_string(),
                ));
            }
            cursor += delta;
            let mut map: Map<BlockId, u64> =
                Map::with_capacity_and_hasher(map_count as usize, BuildMap::default());
            for _ in 0..(map_count as usize) {
                let hash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?);
                cursor += BLOCK_ID_SIZE_BYTES;
                let (period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
                cursor += delta;
                map.insert(hash, period);
            }
            children.push(map);
        }

        // dependencies
        let (dependencies_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        if dependencies_count > max_bootstrap_deps {
            return Err(ModelsError::DeserializeError(
                "too many dependencies to deserialize".to_string(),
            ));
        }
        cursor += delta;
        let mut dependencies = Set::<BlockId>::with_capacity_and_hasher(
            dependencies_count as usize,
            BuildMap::default(),
        );
        for _ in 0..(dependencies_count as usize) {
            let dep = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?);
            cursor += BLOCK_ID_SIZE_BYTES;
            dependencies.insert(dep);
        }

        Ok((
            ExportActiveBlock {
                is_final,
                block_id: block.id,
                block,
                parents,
                children,
                dependencies,
            },
            cursor,
        ))
    }
}
