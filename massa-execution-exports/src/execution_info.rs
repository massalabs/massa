#![allow(dead_code)]

use std::collections::HashMap;

use massa_hash::Hash;
use massa_models::block_id::BlockId;
use massa_models::config::{GENESIS_TIMESTAMP, T0, THREAD_COUNT};
use massa_time::MassaTime;
use schnellru::{ByLength, LruMap};

use massa_deferred_calls::DeferredCall;
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::slot::Slot;
use serde::Serialize;

use crate::types_trace_info::ExecutionResult;
// #[cfg(feature = "execution-trace")]
// use crate::Transfer;

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

/// representation of a transfer
#[derive(Clone, Debug, Serialize)]
pub struct TransferHistory {
    /// sender address
    pub from: Option<Address>,
    /// destination address
    pub to: Option<Address>,
    /// amount of mas
    pub amount: Option<Amount>,
    /// amount of rolls
    pub roll_count: Option<u64>,
    /// Transfer context
    pub context: TransferContext,
    /// Transfer type (mas, roll, deferred-credits)
    pub t_type: TransferType,
    /// Transfer id
    pub id: Option<String>,
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize)]
pub enum TransferType {
    Mas,
    Roll,
    DeferredCredits,
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize)]
pub enum TransferContext {
    TransactionCoins(String),
    AyncMsgCancel,
    DeferredCredits,
    DeferredCallFail,
    DeferredCallCancel,
    DeferredCallCoins,
    DeferredCallRegister,
    DeferredCallStorageRefund,
    OperationFee(String),
    RollBuy(String),
    RollSell(String),
    RollSlash,
    CreateSCStorage,
    DatastoreStorage,
    CallSCCoins(String),
    AsyncMsgCoins,
    EndorsementCreator,
    EndorsementTarget,
    BlockCreatorReward,
    ReadOnlyBytecodeExecutionFee,
    ReadOnlyFunctionCallFee,
    ReadOnlyFunctionCallCoins,
    SetBytecodeStorage,
    AbiCallCoins,
    AbiTransferCoins,
    AbiTransferForCoins,
    AbiSendMsgCoins,
    AbiSendMsgFee,
}

/// Struct for execution info
#[derive(Debug, Clone)]
pub struct ExecutionInfoForSlot {
    /// Slot
    pub slot: Slot,
    /// Timestamp
    pub timestamp: MassaTime,
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
    /// execution trail hash
    pub execution_trail_hash: Hash,
    /// block id (empty if no block)
    pub opt_block_id: Option<String>,
    /// Transfers (empty if execution-info feature is NOT enabled)
    #[cfg(feature = "execution-info")]
    pub transfers: Vec<TransferHistory>,
}

impl ExecutionInfoForSlot {
    /// Create a new ExecutionInfoForSlot structure
    pub fn new(slot: Slot, execution_trail_hash: Hash, block_id: Option<BlockId>) -> Self {
        let timestamp = massa_models::timeslots::get_block_slot_timestamp(
            THREAD_COUNT,
            T0,
            *GENESIS_TIMESTAMP,
            slot,
        )
        .expect("Error getting timestamp for slot in ExecutionInfoForSlot");
        Self {
            slot,
            timestamp,
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
            opt_block_id: block_id.map(|b| b.to_string()),
            #[cfg(feature = "execution-trace")]
            transfers: vec![],
            execution_trail_hash,
        }
    }

    /// When the slot is finalized, we can build the transfer ids
    pub fn build_transfer_ids(&mut self) {
        #[cfg(feature = "execution-info")]
        {
            self.transfers
                .iter_mut()
                .enumerate()
                .for_each(|(index, transfer)| {
                    let id = format!("{}:{}", self.execution_trail_hash, index);
                    transfer.id = Some(id);
                });
        }
    }

    /// Check if the ExecutionInfoForSlot is empty (grpc api use it to return struct or None)
    pub fn is_empty(&self) -> bool {
        let empty = self.denunciations.is_empty()
            && self.operations.is_empty()
            && self.async_messages.is_empty()
            && self.deferred_calls_messages.is_empty()
            && self.deferred_credits_execution.is_empty()
            && self.cancel_async_message_execution.is_empty()
            && self.auto_sell_execution.is_empty()
            && self.endorsement_creator_rewards.is_empty()
            && self.block_producer_reward.is_none()
            && self.endorsement_target_reward.is_none();

        #[cfg(feature = "execution-info")]
        {
            return empty && self.transfers.is_empty();
        }

        #[cfg(not(feature = "execution-info"))]
        {
            empty
        }
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

impl Default for AsyncMessageExecutionResult {
    fn default() -> Self {
        Self::new()
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
    /// coins
    pub coins: Amount,
    /// fee
    pub fee: Amount,
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
            fee: call.fee,
            traces: None,
        }
    }
}
