use massa_models::{
    array_from_slice,
    clique::Clique,
    constants::BLOCK_ID_SIZE_BYTES,
    prehash::{BuildMap, Map, Set},
    with_serialization_context, BlockId, DeserializeCompact, DeserializeVarInt, ModelsError,
    SerializeCompact, SerializeVarInt,
};
use serde::{Deserialize, Serialize};

use crate::{export_active_block::ExportActiveBlock, ledger::LedgerSubset};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapableGraph {
    /// Map of active blocks, were blocks are in their exported version.
    pub active_blocks: Map<BlockId, ExportActiveBlock>,
    /// Best parents hashe in each thread.
    pub best_parents: Vec<(BlockId, u64)>,
    /// Latest final period and block hash in each thread.
    pub latest_final_blocks_periods: Vec<(BlockId, u64)>,
    /// Head of the incompatibility graph.
    pub gi_head: Map<BlockId, Set<BlockId>>,
    /// List of maximal cliques of compatible blocks.
    pub max_cliques: Vec<Clique>,
    /// Ledger at last final blocks
    pub ledger: LedgerSubset,
}

impl SerializeCompact for BootstrapableGraph {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        let (max_bootstrap_blocks, max_bootstrap_cliques) = with_serialization_context(|context| {
            (context.max_bootstrap_blocks, context.max_bootstrap_cliques)
        });

        // active_blocks
        let blocks_count: u32 = self.active_blocks.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many active blocks in BootstrapableGraph: {}",
                err
            ))
        })?;
        if blocks_count > max_bootstrap_blocks {
            return Err(ModelsError::SerializeError(format!("too many blocks in active_blocks for serialization context in BootstrapableGraph: {}", blocks_count)));
        }
        res.extend(blocks_count.to_varint_bytes());
        for (hash, block) in self.active_blocks.iter() {
            res.extend(&hash.to_bytes());
            res.extend(block.to_bytes_compact()?);
        }

        // best_parents
        for (parent_h, parent_period) in self.best_parents.iter() {
            res.extend(&parent_h.to_bytes());
            res.extend(&parent_period.to_varint_bytes());
        }

        // latest_final_blocks_periods
        for (hash, period) in self.latest_final_blocks_periods.iter() {
            res.extend(&hash.to_bytes());
            res.extend(period.to_varint_bytes());
        }

        // gi_head
        let gi_head_count: u32 = self.gi_head.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many gi_head in BootstrapableGraph: {}", err))
        })?;
        res.extend(gi_head_count.to_varint_bytes());
        for (gihash, set) in self.gi_head.iter() {
            res.extend(&gihash.to_bytes());
            let set_count: u32 = set.len().try_into().map_err(|err| {
                ModelsError::SerializeError(format!(
                    "too many entry in gi_head set in BootstrapableGraph: {}",
                    err
                ))
            })?;
            res.extend(set_count.to_varint_bytes());
            for hash in set {
                res.extend(&hash.to_bytes());
            }
        }

        // max_cliques
        let max_cliques_count: u32 = self.max_cliques.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many max_cliques in BootstrapableGraph (format): {}",
                err
            ))
        })?;
        if max_cliques_count > max_bootstrap_cliques {
            return Err(ModelsError::SerializeError(format!(
                "too many max_cliques for serialization context in BootstrapableGraph: {}",
                max_cliques_count
            )));
        }
        res.extend(max_cliques_count.to_varint_bytes());
        for e_clique in self.max_cliques.iter() {
            res.extend(e_clique.to_bytes_compact()?);
        }

        // ledger
        res.extend(self.ledger.to_bytes_compact()?);

        Ok(res)
    }
}

impl DeserializeCompact for BootstrapableGraph {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;
        let (max_bootstrap_blocks, parent_count, max_bootstrap_cliques) =
            with_serialization_context(|context| {
                (
                    context.max_bootstrap_blocks,
                    context.thread_count,
                    context.max_bootstrap_cliques,
                )
            });

        // active_blocks
        let (active_blocks_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        if active_blocks_count > max_bootstrap_blocks {
            return Err(ModelsError::DeserializeError(format!("too many blocks in active_blocks for deserialization context in BootstrapableGraph: {}", active_blocks_count)));
        }
        cursor += delta;
        let mut active_blocks =
            Map::with_capacity_and_hasher(active_blocks_count as usize, BuildMap::default());
        for _ in 0..(active_blocks_count as usize) {
            let hash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += BLOCK_ID_SIZE_BYTES;
            let (block, delta) = ExportActiveBlock::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            active_blocks.insert(hash, block);
        }

        // best_parents
        let mut best_parents: Vec<(BlockId, u64)> = Vec::with_capacity(parent_count as usize);
        for _ in 0..parent_count {
            let parent_h = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += BLOCK_ID_SIZE_BYTES;
            let (parent_period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            best_parents.push((parent_h, parent_period));
        }

        // latest_final_blocks_periods
        let mut latest_final_blocks_periods: Vec<(BlockId, u64)> =
            Vec::with_capacity(parent_count as usize);
        for _ in 0..parent_count {
            let hash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += BLOCK_ID_SIZE_BYTES;
            let (period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            latest_final_blocks_periods.push((hash, period));
        }

        // gi_head
        let (gi_head_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        if gi_head_count > max_bootstrap_blocks {
            return Err(ModelsError::DeserializeError(format!(
                "too many blocks in gi_head for deserialization context in BootstrapableGraph: {}",
                gi_head_count
            )));
        }
        cursor += delta;
        let mut gi_head =
            Map::with_capacity_and_hasher(gi_head_count as usize, BuildMap::default());
        for _ in 0..(gi_head_count as usize) {
            let gihash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += BLOCK_ID_SIZE_BYTES;
            let (set_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
            if set_count > max_bootstrap_blocks {
                return Err(ModelsError::DeserializeError(format!("too many blocks in a set in gi_head for deserialization context in BootstrapableGraph: {}", set_count)));
            }
            cursor += delta;
            let mut set =
                Set::<BlockId>::with_capacity_and_hasher(set_count as usize, BuildMap::default());
            for _ in 0..(set_count as usize) {
                let hash = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += BLOCK_ID_SIZE_BYTES;
                set.insert(hash);
            }
            gi_head.insert(gihash, set);
        }

        // max_cliques
        let (max_cliques_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        if max_cliques_count > max_bootstrap_cliques {
            return Err(ModelsError::DeserializeError(format!(
                "too many for deserialization context in BootstrapableGraph: {}",
                max_cliques_count
            )));
        }
        cursor += delta;
        let mut max_cliques: Vec<Clique> = Vec::with_capacity(max_cliques_count as usize);
        for _ in 0..max_cliques_count {
            let (c, delta) = Clique::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            max_cliques.push(c);
        }

        // ledger
        let (ledger, delta) = LedgerSubset::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        Ok((
            BootstrapableGraph {
                active_blocks,
                best_parents,
                latest_final_blocks_periods,
                gi_head,
                max_cliques,
                ledger,
            },
            cursor,
        ))
    }
}
