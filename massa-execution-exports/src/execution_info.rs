#![allow(dead_code)]

use std::collections::HashMap;

use schnellru::{ByLength, LruMap};

use massa_deferred_calls::DeferredCall;
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::slot::Slot;

use crate::types_trace_info::ExecutionResult;

/// Struct for Execution info per slot
pub struct ExecutionInfo {
    /// Map of execution info
    pub info_per_slot: LruMap<Slot, ExecutionInfoForSlot>,
}

impl ExecutionInfo {
    /// Create a new ExecutionInfo
    pub fn new(max_slot_size_cache: u32) -> Self {
        Self {
            info_per_slot: LruMap::new(ByLength::new(max_slot_size_cache)),
        }
    }

    /// Save transfer for a given slot
    pub fn save_for_slot(&mut self, slot: Slot, info: ExecutionInfoForSlot) {
        self.info_per_slot.insert(slot, info);
    }
}

/// Struct to store Roll related operation
#[derive(Debug, Clone)]
pub enum OperationInfo {
    /// Roll buy amount
    RollBuy(Address, u64),
    /// Roll sell amount
    RollSell(Address, u64),
}

/// Struct for execution info
#[derive(Debug, Clone)]
pub struct ExecutionInfoForSlot {
    /// Reward for block producer
    pub block_producer_reward: Option<(Address, Amount)>,
    /// Rewards for endorsmement creators
    pub endorsement_creator_rewards: HashMap<Address, Amount>,
    /// Reward for endorsement target
    pub endorsement_target_reward: Option<(Address, Amount)>,
    /// Executed denunciation
    pub denunciations: Vec<Result<DenunciationResult, String>>,
    /// Executed Roll buy / sell
    pub operations: Vec<OperationInfo>,
    /// Executed Async message
    pub async_messages: Vec<Result<AsyncMessageExecutionResult, String>>,
    /// Executed Deferred calls
    pub deferred_calls_messages: Vec<Result<DeferredCallExecutionResult, String>>,
    /// Deferred credits execution (empty if execution-info feature is NOT enabled)
    pub deferred_credits_execution: Vec<(Address, Result<Amount, String>)>,
    /// Cancel async message execution (empty if execution-info feature is NOT enabled)
    pub cancel_async_message_execution: Vec<(Address, Result<Amount, String>)>,
    /// Auto sell roll execution (empty if execution-info feature is NOT enabled)
    pub auto_sell_execution: Vec<(Address, Amount)>,
}

impl ExecutionInfoForSlot {
    /// Create a new ExecutionInfoForSlot structure
    pub fn new() -> Self {
        Self {
            block_producer_reward: None,
            endorsement_creator_rewards: Default::default(),
            endorsement_target_reward: None,
            denunciations: Default::default(),
            operations: Default::default(),
            async_messages: Default::default(),
            deferred_calls_messages: Default::default(),
            deferred_credits_execution: vec![],
            cancel_async_message_execution: vec![],
            auto_sell_execution: vec![],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.block_producer_reward.is_none()
            && self.endorsement_creator_rewards.is_empty()
            && self.endorsement_target_reward.is_none()
            && self.denunciations.is_empty()
            && self.operations.is_empty()
            && self.async_messages.is_empty()
            && self.deferred_calls_messages.is_empty()
            && self.deferred_credits_execution.is_empty()
            && self.cancel_async_message_execution.is_empty()
            && self.auto_sell_execution.is_empty()
    }
}

/// structure describing the output of a denunciation execution
#[derive(Debug, Clone)]
pub struct DenunciationResult {
    /// Target address of the denunciation
    pub address_denounced: Address,
    /// Denunciation slot
    pub slot: Slot,
    /// Amount slashed if successfully executed
    pub slashed: Amount,
}

/// An async message execution result
#[derive(Debug, Clone)]
pub struct AsyncMessageExecutionResult {
    /// Execution success or not
    pub success: bool,
    /// Sender address
    pub sender: Option<Address>,
    /// Destination address
    pub destination: Option<Address>,
    /// Amount
    pub coins: Option<Amount>,
    /// Traces
    pub traces: Option<ExecutionResult>,
}

impl AsyncMessageExecutionResult {
    /// Create a new AsyncMessageExecutionResult structure
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

/// Deferred call execution result
#[derive(Debug, Clone)]
pub struct DeferredCallExecutionResult {
    /// Execution success or not
    pub success: bool,
    /// sender address
    pub sender: Address,
    /// target address
    pub target_address: Address,
    pub(crate) target_function: String,
    pub(crate) coins: Amount,
    /// traces
    pub traces: Option<ExecutionResult>,
}

impl DeferredCallExecutionResult {
    /// Create a new DeferredCallExecutionResult structure
    pub fn new(call: &DeferredCall) -> Self {
        Self {
            success: false,
            sender: call.sender_address,
            target_address: call.target_address,
            target_function: call.target_function.clone(),
            coins: call.coins,
            traces: None,
        }
    }
}
