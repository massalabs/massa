// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    block_id::BlockId,
    endorsement::{EndorsementId, SecureShareEndorsement},
};
use serde::{Deserialize, Serialize};

use crate::display_if_true;

/// All you wanna know about an endorsement
#[derive(Debug, Deserialize, Serialize, Clone)]
pub(crate)  struct EndorsementInfo {
    /// id
    pub(crate)  id: EndorsementId,
    /// true if endorsement is still in pool
    pub(crate)  in_pool: bool,
    /// the endorsement appears in `in_blocks`
    /// if it appears in multiple blocks, these blocks are in different cliques
    pub(crate)  in_blocks: Vec<BlockId>,
    /// true if the endorsement is final (for example in a final block)
    pub(crate)  is_final: bool,
    /// the endorsement itself
    pub(crate)  endorsement: SecureShareEndorsement,
}

impl std::fmt::Display for EndorsementInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Endorsement {}{}{}",
            self.id,
            display_if_true(self.in_pool, " (in pool)"),
            display_if_true(self.is_final, " (final)")
        )?;
        writeln!(f, "In blocks:")?;
        for block_id in &self.in_blocks {
            writeln!(f, "\t- {}", block_id)?;
        }
        writeln!(f, "{}", self.endorsement)?;
        Ok(())
    }
}
