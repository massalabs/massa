// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{address::Address, block::Block, block_id::BlockId, slot::Slot};

use serde::{Deserialize, Serialize};

use crate::display_if_true;

/// refactor to delete
#[derive(Debug, Deserialize, Serialize)]
pub(crate)  struct BlockInfo {
    /// block id
    pub(crate)  id: BlockId,
    /// optional block info content
    pub(crate)  content: Option<BlockInfoContent>,
}

/// Block content
#[derive(Debug, Deserialize, Serialize)]
pub(crate)  struct BlockInfoContent {
    /// true if final
    pub(crate)  is_final: bool,
    /// true if in the greatest clique (and not final)
    pub(crate)  is_in_blockclique: bool,
    /// true if candidate (active any clique but not final)
    pub(crate)  is_candidate: bool,
    /// true if discarded
    pub(crate)  is_discarded: bool,
    /// block
    pub(crate)  block: Block,
}

impl std::fmt::Display for BlockInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(content) = &self.content {
            writeln!(
                f,
                "Block ID: {}{}{}{}{}",
                self.id,
                display_if_true(content.is_final, " (final)"),
                display_if_true(content.is_candidate, " (candidate)"),
                display_if_true(content.is_in_blockclique, " (blockclique)"),
                display_if_true(content.is_discarded, " (discarded)"),
            )?;
            writeln!(f, "Block: {}", content.block)?;
        } else {
            writeln!(f, "Block {} not found", self.id)?;
        }
        Ok(())
    }
}

/// A block resume (without the block itself)
#[derive(Debug, Deserialize, Serialize)]
pub(crate)  struct BlockSummary {
    /// id
    pub(crate)  id: BlockId,
    /// true if in a final block
    pub(crate)  is_final: bool,
    /// true if incompatible with a final block
    pub(crate)  is_stale: bool,
    /// true if in the greatest block clique
    pub(crate)  is_in_blockclique: bool,
    /// the slot the block is in
    pub(crate)  slot: Slot,
    /// the block creator
    pub(crate)  creator: Address,
    /// the block parents
    pub(crate)  parents: Vec<BlockId>,
}

impl std::fmt::Display for BlockSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Block's ID: {}{}{}{}",
            self.id,
            display_if_true(self.is_final, "final"),
            display_if_true(self.is_stale, "stale"),
            display_if_true(self.is_in_blockclique, "in blockclique"),
        )?;
        writeln!(f, "Slot: {}", self.slot)?;
        writeln!(f, "Creator: {}", self.creator)?;
        writeln!(f, "Parents' IDs:")?;
        for parent in &self.parents {
            writeln!(f, "\t- {}", parent)?;
        }
        Ok(())
    }
}
