// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    block_id::BlockId,
    operation::{OperationId, SecureShareOperation},
};

use massa_signature::{PublicKey, Signature};
use serde::{Deserialize, Serialize};

use crate::display_if_true;

/// operation input
#[derive(Serialize, Deserialize, Debug)]
pub struct OperationInput {
    /// The public key of the creator of the TX
    pub creator_public_key: PublicKey,
    /// The signature of the operation
    pub signature: Signature,
    /// The serialized version of the content `base58` encoded
    pub serialized_content: Vec<u8>,
}

/// Operation and contextual info about it
#[derive(Debug, Deserialize, Serialize)]
pub struct OperationInfo {
    /// id
    pub id: OperationId,
    /// true if operation is still in pool
    pub in_pool: bool,
    /// the operation appears in `in_blocks`
    /// if it appears in multiple blocks, these blocks are in different cliques
    pub in_blocks: Vec<BlockId>,
    /// true if the operation is final (for example in a final block)
    pub is_final: bool,
    /// the operation itself
    pub operation: SecureShareOperation,
}

impl std::fmt::Display for OperationInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Operation {}{}{}",
            self.id,
            display_if_true(self.in_pool, " (in pool)"),
            display_if_true(self.is_final, " (final)")
        )?;
        writeln!(f, "In blocks:")?;
        for block_id in &self.in_blocks {
            writeln!(f, "\t- {}", block_id)?;
        }
        writeln!(f, "{}", self.operation)?;
        Ok(())
    }
}
