// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::fmt::Display;

use super::operation::Operation;
use crate::prehash::Map;
use crate::{Address, BlockId};
use massa_signature::{PublicKey, Signature};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationSearchResultBlockStatus {
    Incoming,
    WaitingForSlot,
    WaitingForDependencies,
    Active,
    Discarded,
    Stored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationSearchResultStatus {
    Pending,
    InBlock(OperationSearchResultBlockStatus),
    Discarded,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationSearchResult {
    pub op: Operation,
    pub in_pool: bool,
    pub in_blocks: Map<BlockId, (usize, bool)>, // index, is_final
    pub status: OperationSearchResultStatus,
}

impl OperationSearchResult {
    pub fn extend(&mut self, other: &OperationSearchResult) {
        self.in_pool = self.in_pool || other.in_pool;
        self.in_blocks.extend(other.in_blocks.iter());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StakersCycleProductionStats {
    pub cycle: u64,
    pub is_final: bool,
    pub ok_nok_counts: Map<Address, (u64, u64)>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PubkeySig {
    pub public_key: PublicKey,
    pub signature: Signature,
}

impl Display for PubkeySig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Public key: {}", self.public_key)?;
        writeln!(f, "Signature: {}", self.signature)
    }
}
