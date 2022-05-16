// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::prehash::Map;
use crate::{Address, BlockId, SignedOperation};
use massa_signature::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use std::fmt::Display;

/// Status in which an operation can be (derived from the block status)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationSearchResultBlockStatus {
    /// the block hasn't been processed by consensus yet
    Incoming,
    /// the block waits for it's slot for further processing
    WaitingForSlot,
    /// the block waits for dependencies for further processing
    WaitingForDependencies,
    /// the block has been processed and is valid
    Active,
    /// the block is discarded
    Discarded,
}

/// Status in which an operation can be
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationSearchResultStatus {
    /// in pool
    Pending,
    /// in a block, the block being in `[OperationSearchResultBlockStatus]`
    InBlock(OperationSearchResultBlockStatus),
    /// discarded
    Discarded,
}

/// operation info
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationSearchResult {
    /// the operation
    pub op: SignedOperation,
    /// true if in pool
    pub in_pool: bool,
    /// maps block id to index on the operation in the block and if it's final
    pub in_blocks: Map<BlockId, (usize, bool)>,
    /// operation status
    pub status: OperationSearchResultStatus,
}

impl OperationSearchResult {
    /// combine two operation search result
    pub fn extend(&mut self, other: &OperationSearchResult) {
        self.in_pool = self.in_pool || other.in_pool;
        self.in_blocks.extend(other.in_blocks.iter());
    }
}

/// all the production stats for every known staker
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StakersCycleProductionStats {
    /// cycle number
    pub cycle: u64,
    /// if that cycle is final
    pub is_final: bool,
    /// map address to produced valid block count and not valid but expected block count
    /// really a re arranged `[crate::address::AddressCycleProductionStats]`
    pub ok_nok_counts: Map<Address, (u64, u64)>,
}

/// just a public key and a signature it has produced
/// used for serialization/deserialization purpose
#[derive(Debug, Serialize, Deserialize)]
pub struct PubkeySig {
    /// public key
    pub public_key: PublicKey,
    /// signature
    pub signature: Signature,
}

impl Display for PubkeySig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Public key: {}", self.public_key)?;
        writeln!(f, "Signature: {}", self.signature)
    }
}
