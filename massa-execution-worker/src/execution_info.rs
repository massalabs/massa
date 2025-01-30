#![allow(dead_code)]
// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! # General description
//!
//! 'execution-info' is a Rust feature that can be enabled for the execution module to provide
//! various execution information
//!
//! Note:
//! * enabling the 'execution-info' feature also enables the 'execution-trace' feature.
//!
//! # Information collected during execution
//!
//! * Roll:
//!   * Roll buy
//!   * Roll sell
//!   * Auto sell
//! * Denunciations execution (aka slash)
//! * Rewards
//!   * endorsement creator reward
//!   * endorsement target reward
//!   * block producer reward
//! * Deferred credits execution
//! * Async message execution + cancellation
//!
//! # Usage
//!
//! The 'execution-info' feature was originally developed for the [slot-replayer](https://github.com/massalabs/massa-slot-replayer) project.
//!
//! By running a massa node with the Rust feature 'dump-block', the executed blocks will be stored on disk (files or rocksdb).
//!
//! Then the slot-replayer use the massa-execution-module to replay those blocks and use the 'execution-info' + 'slot-replayer' features
//! to print the result of each execution (see execution.rs, method: apply_final_execution_output).
//!  
//! Note: no json-rpc or grpc endpoint is available to query 'execution-info' result.
//!
//! # Test
//!
//! The functional test `test_local_sandbox_op_with_backup` shows how to backup blocks.

use std::collections::HashMap;

use schnellru::{ByLength, LruMap};
// use massa_execution_exports::Transfer;

use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::slot::Slot;

use crate::execution::ExecutionResult;

pub struct ExecutionInfo {
    info_per_slot: LruMap<Slot, ExecutionInfoForSlot>,
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
