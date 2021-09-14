// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::operation::Operation;
use crate::{address::AddressHashMap, BlockHashMap};
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
    pub in_blocks: BlockHashMap<(usize, bool)>, // index, is_final
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
    pub ok_nok_counts: AddressHashMap<(u64, u64)>,
}
