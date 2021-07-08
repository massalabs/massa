use super::block::BlockId;
use super::operation::Operation;
use crate::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    pub in_blocks: HashMap<BlockId, (usize, bool)>, // index, is_final
    pub status: OperationSearchResultStatus,
}

impl OperationSearchResult {
    pub fn extend(&mut self, other: &OperationSearchResult) {
        self.in_pool = self.in_pool || other.in_pool;
        self.in_blocks.extend(other.in_blocks.iter());
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AddressRollState {
    pub final_rolls: u64,
    pub active_rolls: Option<u64>,
    pub candidate_rolls: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddressesRollState {
    pub states: HashMap<Address, AddressRollState>,
}
