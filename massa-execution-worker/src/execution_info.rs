#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;

use schnellru::{ByLength, LruMap};
// use massa_execution_exports::Transfer;

use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::slot::Slot;

use crate::execution::ExecutionResult;

pub struct ExecutionInfo {
    info_per_slot: LruMap<Slot, ExecutionInfoForSlot>,
}

impl fmt::Debug for ExecutionInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutionInfo")
            .field("peek newest", &self.info_per_slot.peek_newest())
            .finish()
    }
}


impl ExecutionInfo {
    pub(crate) fn new(max_slot_size_cache: u32) -> Self {
        Self {
            info_per_slot: LruMap::new(ByLength::new(max_slot_size_cache)),
        }
    }

    /// Save transfer for a given slot
    pub(crate) fn save_for_slot(&mut self, slot: Slot, info: ExecutionInfoForSlot) {
        self.info_per_slot.insert(slot, info);
    }
}

#[derive(Debug)]
pub enum OperationInfo {
    RollBuy(u64),
    RollSell(u64),
}

#[derive(Debug)]
pub struct ExecutionInfoForSlot {
    pub(crate) block_producer_reward: Option<(Address, Amount)>,
    pub(crate) endorsement_creator_rewards: HashMap<Address, Amount>,
    pub(crate) endorsement_target_reward: Option<(Address, Amount)>,
    pub(crate) denunciations: Vec<Result<DenunciationResult, String>>,
    pub(crate) operations: Vec<OperationInfo>,
    pub(crate) async_messages: Vec<Result<AsyncMessageExecutionResult, String>>,
    /// Deferred credits execution (empty if execution-info feature is NOT enabled)
    pub deferred_credits_execution: Vec<(Address, Result<Amount, String>)>,
    /// Cancel async message execution (empty if execution-info feature is NOT enabled)
    pub cancel_async_message_execution: Vec<(Address, Result<Amount, String>)>,
    /// Auto sell roll execution (empty if execution-info feature is NOT enabled)
    pub auto_sell_execution: Vec<(Address, Amount)>,
}

impl ExecutionInfoForSlot {
    pub fn new() -> Self {
        Self {
            block_producer_reward: None,
            endorsement_creator_rewards: Default::default(),
            endorsement_target_reward: None,
            denunciations: Default::default(),
            operations: Default::default(),
            async_messages: Default::default(),
            deferred_credits_execution: vec![],
            cancel_async_message_execution: vec![],
            auto_sell_execution: vec![],
        }
    }
}

/// structure describing the output of a denunciation execution
#[derive(Debug)]
pub struct DenunciationResult {
    /// Target address of the denunciation
    pub address_denounced: Address,
    /// Denunciation slot
    pub slot: Slot,
    /// Amount slashed if successfully executed
    pub slashed: Amount,
}

#[derive(Debug)]
pub struct AsyncMessageExecutionResult {
    pub(crate) success: bool,
    pub(crate) sender: Option<Address>,
    pub(crate) destination: Option<Address>,
    pub(crate) coins: Option<Amount>,
    pub(crate) traces: Option<ExecutionResult>,
}

impl AsyncMessageExecutionResult {
    pub fn new() -> Self {
        Self {
            success: false,
            sender: None,
            destination: None,
            coins: None,
            traces: None,
        }
    }
}
