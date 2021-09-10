// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{
    array_from_slice, u8_from_slice, with_serialization_context, BlockId, DeserializeCompact,
    DeserializeVarInt, ModelsError, SerializeCompact, SerializeVarInt, BLOCK_ID_SIZE_BYTES,
};
use core::usize;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::convert::TryInto;

impl<'a> From<&'a ExportClique> for Clique {
    fn from(export_clique: &'a ExportClique) -> Self {
        Clique {
            block_ids: export_clique.block_ids.iter().cloned().collect(),
            fitness: export_clique.fitness,
            is_blockclique: export_clique.is_blockclique,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clique {
    pub block_ids: HashSet<BlockId>,
    pub fitness: u64,
    pub is_blockclique: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportClique {
    pub block_ids: Vec<BlockId>,
    pub fitness: u64,
    pub is_blockclique: bool,
}

impl<'a> From<&'a Clique> for ExportClique {
    fn from(clique: &'a Clique) -> Self {
        ExportClique {
            block_ids: clique.block_ids.iter().cloned().collect(),
            fitness: clique.fitness,
            is_blockclique: clique.is_blockclique,
        }
    }
}

impl SerializeCompact for ExportClique {
    /// ## Example
    /// ```rust
    /// use models::clique::ExportClique;
    /// # use models::{SerializeCompact, DeserializeCompact, SerializationContext, BlockId};
    /// # use crypto::hash::Hash;
    /// # use std::str::FromStr;
    /// # models::init_serialization_context(models::SerializationContext {
    /// #     max_block_operations: 1024,
    /// #     parent_count: 2,
    /// #     max_peer_list_length: 128,
    /// #     max_message_size: 3 * 1024 * 1024,
    /// #     max_block_size: 3 * 1024 * 1024,
    /// #     max_bootstrap_blocks: 100,
    /// #     max_bootstrap_cliques: 100,
    /// #     max_bootstrap_deps: 100,
    /// #     max_bootstrap_children: 100,
    /// #     max_ask_blocks_per_message: 10,
    /// #     max_operations_per_message: 1024,
    /// #     max_endorsements_per_message: 1024,
    /// #     max_bootstrap_message_size: 100000000,
    /// #     max_bootstrap_pos_cycles: 10000,
    /// #     max_bootstrap_pos_entries: 10000,
    /// #     max_block_endorsments: 8,
    /// # });
    /// # pub fn get_dummy_block_id(s: &str) -> BlockId {
    /// #     BlockId(Hash::hash(s.as_bytes()))
    /// # }
    /// let clique = ExportClique {
    ///         block_ids: vec![get_dummy_block_id("parent1"), get_dummy_block_id("parent2")],
    ///         fitness: 123,
    ///         is_blockclique: true,
    ///     };
    /// let bytes = clique.clone().to_bytes_compact().unwrap();
    /// let (res, _) = ExportClique::from_bytes_compact(&bytes).unwrap();
    /// assert_eq!(clique.block_ids, res.block_ids);
    /// assert_eq!(clique.is_blockclique, res.is_blockclique);
    /// assert_eq!(clique.fitness, res.fitness);
    /// ```
    fn to_bytes_compact(&self) -> Result<Vec<u8>, crate::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // block_ids
        let block_ids_count: u32 = self.block_ids.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!("too many blocks in in clique: {:?}", err))
        })?;
        res.extend(&block_ids_count.to_varint_bytes());
        for b_id in self.block_ids.iter() {
            res.extend(&b_id.to_bytes());
        }

        // fitness
        res.extend(&self.fitness.to_varint_bytes());

        // is_blockclique
        res.push(if self.is_blockclique { 1u8 } else { 0u8 });

        Ok(res)
    }
}

impl DeserializeCompact for ExportClique {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), crate::ModelsError> {
        let mut cursor = 0usize;
        let max_bootstrap_blocks =
            with_serialization_context(|context| context.max_bootstrap_blocks);

        // block_ids
        let (block_count, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        if block_count > max_bootstrap_blocks {
            return Err(ModelsError::DeserializeError(format!(
                "too many blocks in clique for deserialization"
            )));
        }
        cursor += delta;
        let mut block_ids: Vec<BlockId> = Vec::with_capacity(block_count as usize);
        for _ in 0..block_count {
            let b_id = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += BLOCK_ID_SIZE_BYTES;
            block_ids.push(b_id);
        }

        // fitness
        let (fitness, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // is_blockclique
        let is_blockclique = match u8_from_slice(&buffer[cursor..])? {
            0u8 => false,
            1u8 => true,
            _ => {
                return Err(ModelsError::SerializeError(
                    "could not deserialize active_block.production_events.has_created".into(),
                ))
            }
        };
        cursor += 1;

        Ok((
            ExportClique {
                block_ids,
                fitness,
                is_blockclique,
            },
            cursor,
        ))
    }
}
