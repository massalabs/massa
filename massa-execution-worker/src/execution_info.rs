#![allow(dead_code)]

use std::collections::HashMap;

use crate::execution::ExecutionResult;
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::slot::Slot;

pub struct ExecutionInfo {}

impl ExecutionInfo {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

pub enum OperationInfo {
    RollBuy(u64),
    RollSell(u64),
}

pub struct ExecutionInfoForSlot {
    pub(crate) block_producer_reward: Option<(Address, Amount)>,
    pub(crate) endorsement_creator_rewards: HashMap<Address, Amount>,
    pub(crate) endorsement_target_reward: Option<(Address, Amount)>,
    pub(crate) denunciations: Vec<Result<DenunciationResult, String>>,
    pub(crate) operations: Vec<OperationInfo>,
    pub(crate) async_messages: Vec<Result<AsyncMessageExecutionResult, String>>,
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
