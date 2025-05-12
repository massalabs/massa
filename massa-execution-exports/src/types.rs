// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file exports useful types used to interact with the execution worker

use crate::error::ExecutionQueryError;
use crate::event_store::EventStore;
use massa_deferred_calls::DeferredCall;
use massa_final_state::StateChanges;
use massa_hash::Hash;
use massa_models::block_id::BlockId;
use massa_models::bytecode::Bytecode;
use massa_models::datastore::Datastore;
use massa_models::deferred_calls::DeferredCallId;
use massa_models::denunciation::DenunciationIndex;
use massa_models::execution::EventFilter;
use massa_models::operation::OperationId;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::PreHashSet;
use massa_models::{
    address::Address, address::ExecutionAddressCycleInfo, amount::Amount, slot::Slot,
};
use massa_pos_exports::ProductionStats;
use massa_storage::Storage;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(feature = "execution-trace")]
use crate::types_trace_info::{SlotAbiCallStack, Transfer};

/// Metadata needed to execute the block
#[derive(Clone, Debug)]
pub struct ExecutionBlockMetadata {
    /// Address of the creator of the parent in the same thread
    pub same_thread_parent_creator: Option<Address>,
    /// Storage referencing the block and its contents
    pub storage: Option<Storage>,
}

/// Request to atomically execute a batch of execution state queries
pub struct ExecutionQueryRequest {
    /// List of requests
    pub requests: Vec<ExecutionQueryRequestItem>,
}

/// Response to a list of execution queries
pub struct ExecutionQueryResponse {
    /// List of responses
    pub responses: Vec<Result<ExecutionQueryResponseItem, ExecutionQueryError>>,
    /// Last executed candidate slot
    pub candidate_cursor: Slot,
    /// Last executed final slot
    pub final_cursor: Slot,
    /// Final state hash
    pub final_state_fingerprint: Hash,
}

/// Execution state query item
pub enum ExecutionQueryRequestItem {
    /// checks if address exists (candidate) returns ExecutionQueryResponseItem::Boolean(true) if it does
    AddressExistsCandidate(Address),
    /// checks if address exists (final) returns ExecutionQueryResponseItem::Boolean(true) if it does
    AddressExistsFinal(Address),
    /// gets the balance (candidate) of an address, returns ExecutionQueryResponseItem::Amount(balance) or an error if the address is not found
    AddressBalanceCandidate(Address),
    /// gets the balance (final) of an address, returns ExecutionQueryResponseItem::Amount(balance) or an error if the address is not found
    AddressBalanceFinal(Address),
    /// gets the bytecode (candidate) of an address, returns ExecutionQueryResponseItem::Bytecode(bytecode) or an error if the address is not found
    AddressBytecodeCandidate(Address),
    /// gets the bytecode (final) of an address, returns ExecutionQueryResponseItem::Bytecode(bytecode) or an error if the address is not found
    AddressBytecodeFinal(Address),
    /// gets the datastore keys (candidate) of an address, returns ExecutionQueryResponseItem::KeyList(keys) or an error if the address is not found
    AddressDatastoreKeysCandidate {
        /// Address for which to query the datastore
        addr: Address,
        /// Filter only entries whose key starts with a prefix
        prefix: Vec<u8>,
    },
    /// gets the datastore keys (final) of an address, returns ExecutionQueryResponseItem::KeyList(keys) or an error if the address is not found
    AddressDatastoreKeysFinal {
        /// Address for which to query the datastore
        addr: Address,
        /// Filter only entries whose key starts with a prefix
        prefix: Vec<u8>,
    },
    /// gets a datastore value (candidate) for an address, returns ExecutionQueryResponseItem::DatastoreValue(keys) or an error if the address or key is not found
    AddressDatastoreValueCandidate {
        /// Address for which to query the datastore
        addr: Address,
        /// Key of the entry
        key: Vec<u8>,
    },
    /// gets a datastore value (final) for an address, returns ExecutionQueryResponseItem::DatastoreValue(keys) or an error if the address or key is not found
    AddressDatastoreValueFinal {
        /// Address for which to query the datastore
        addr: Address,
        /// Key of the entry
        key: Vec<u8>,
    },

    /// gets the execution status (candidate) for an operation, returns ExecutionQueryResponseItem::ExecutionStatus(status)
    OpExecutionStatusCandidate(OperationId),
    /// gets the execution status (final) for an operation, returns ExecutionQueryResponseItem::ExecutionStatus(status)
    OpExecutionStatusFinal(OperationId),

    /// gets the deferred call quote (candidate) for a slot, returns ExecutionQueryResponseItem::DeferredCallQuote(available, price)
    DeferredCallQuote {
        /// slot to query
        target_slot: Slot,
        /// gas request
        max_gas_request: u64,
        /// params size
        params_size: u64,
    },
    /// get info of deferred calls
    DeferredCallInfo(DeferredCallId),
    /// retrieves the deferred call for given slot
    DeferredCallsBySlot(Slot),

    /// gets the execution status (candidate) for an denunciation, returns ExecutionQueryResponseItem::ExecutionStatus(status)
    DenunciationExecutionStatusCandidate(DenunciationIndex),
    /// gets the execution status (final) for an denunciation, returns ExecutionQueryResponseItem::ExecutionStatus(status)
    DenunciationExecutionStatusFinal(DenunciationIndex),

    /// gets the roll count (candidate) of an address, returns ExecutionQueryResponseItem::RollCount(rolls) or an error if the address is not found
    AddressRollsCandidate(Address),
    /// gets the roll count (final) of an address, returns ExecutionQueryResponseItem::RollCount(rolls) or an error if the address is not found
    AddressRollsFinal(Address),
    /// gets the deferred credits (candidate) of an address, returns ExecutionQueryResponseItem::DeferredCredits(deferred_credits) or an error if the address is not found
    AddressDeferredCreditsCandidate(Address),
    /// gets the deferred credits (final) of an address, returns ExecutionQueryResponseItem::DeferredCredits(deferred_credits) or an error if the address is not found
    AddressDeferredCreditsFinal(Address),

    /// get all information for a given cycle, returns ExecutionQueryResponseItem::CycleInfos(cycle_infos) or an error if the cycle is not found
    CycleInfos {
        /// cycle to query
        cycle: u64,
        /// optionally restrict the query to a set of addresses. If None, the info for all addresses will be returned.
        restrict_to_addresses: Option<PreHashSet<Address>>,
    },

    /// get filtered events. Returns ExecutionQueryResponseItem::Events
    Events(EventFilter),
}

/// Execution state query response item
pub enum ExecutionQueryResponseItem {
    /// boolean value
    Boolean(bool),
    /// roll counts value
    RollCount(u64),
    /// amount value
    Amount(Amount),
    /// bytecode
    Bytecode(Bytecode),
    /// datastore value
    DatastoreValue(Vec<u8>),
    /// list of keys
    KeyList(BTreeSet<Vec<u8>>),
    /// deferred call quote (target_slot, gas_request, available, price)
    DeferredCallQuote(Slot, u64, bool, Amount),
    /// deferred call info value
    DeferredCallInfo(DeferredCallId, DeferredCall),
    /// deferred call slot calls value
    DeferredCallsBySlot(Slot, Vec<DeferredCallId>),
    /// deferred credits value
    DeferredCredits(BTreeMap<Slot, Amount>),
    /// execution status value
    ExecutionStatus(ExecutionQueryExecutionStatus),
    /// cycle infos value
    CycleInfos(ExecutionQueryCycleInfos),
    /// Events
    Events(Vec<SCOutputEvent>),
}

/// Execution status of an operation or denunciation
pub enum ExecutionQueryExecutionStatus {
    /// The operation or denunciation was found as successfully executed in the active history
    AlreadyExecutedWithSuccess,
    /// The operation or denunciation was found as executed with errors in the active history
    AlreadyExecutedWithFailure,
    /// No information about the operation or denunciation execution were found in the node.
    /// However the node only keeps execution information until the operation or denunciation expires
    /// in order to prevent it from being re-executed during its validity time.
    /// ExecutableOrExpired means that the operation or denunciations was either never executed,
    /// or was executed previously and ran out of its validify period.
    /// In other terms, the operation or denunciation can still be executed unless it has expired.
    ExecutableOrExpired,
}

/// Information about cycles
pub struct ExecutionQueryCycleInfos {
    /// cycle number
    pub cycle: u64,
    /// whether the cycle is final
    pub is_final: bool,
    /// infos for each PoS-participating address among the ones that were asked
    pub staker_infos: BTreeMap<Address, ExecutionQueryStakerInfo>,
}

/// Staker information for a given cycle
pub struct ExecutionQueryStakerInfo {
    /// active roll count
    pub active_rolls: u64,
    /// production stats
    pub production_stats: ProductionStats,
}

/// Execution info about an address
#[derive(Clone, Debug)]
pub struct ExecutionAddressInfo {
    /// candidate balance of the address
    pub candidate_balance: Amount,
    /// final balance of the address
    pub final_balance: Amount,

    /// final number of rolls the address has
    pub final_roll_count: u64,
    /// final datastore keys of the address
    pub final_datastore_keys: BTreeSet<Vec<u8>>,

    /// candidate number of rolls the address has
    pub candidate_roll_count: u64,
    /// candidate datastore keys of the address
    pub candidate_datastore_keys: BTreeSet<Vec<u8>>,

    /// future deferred credits
    pub future_deferred_credits: BTreeMap<Slot, Amount>,

    /// cycle information
    pub cycle_infos: Vec<ExecutionAddressCycleInfo>,
}

/// structure describing the output of the execution of a slot
#[derive(Debug, Clone)]
pub enum SlotExecutionOutput {
    /// Executed slot output
    ExecutedSlot(ExecutionOutput),

    /// Finalized slot output
    FinalizedSlot(ExecutionOutput),
}

/// structure storing a block id + network versions (from a block header)
#[derive(Debug, Clone, Serialize)]
pub struct ExecutedBlockInfo {
    /// Block id
    pub block_id: BlockId,
    /// Current network version (see Versioning doc)
    pub current_version: u32,
    /// Announced network version (see Versioning doc)
    pub announced_version: Option<u32>,
}

/// structure describing the output of a single execution
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionOutput {
    /// slot
    pub slot: Slot,
    /// optional executed block info at that slot (None if miss)
    pub block_info: Option<ExecutedBlockInfo>,
    /// state changes caused by the execution step
    pub state_changes: StateChanges,
    /// events emitted by the execution step
    pub events: EventStore,
    /// slot trace
    #[cfg(feature = "execution-trace")]
    pub slot_trace: Option<(SlotAbiCallStack, Vec<Transfer>)>,
    /// storage
    #[cfg(feature = "dump-block")]
    #[serde(skip_serializing)]
    pub storage: Option<Storage>,
    /// Deferred credits execution (empty if execution-info feature is NOT enabled)
    pub deferred_credits_execution: Vec<(Address, Result<Amount, String>)>,
    /// Cancel async message execution (empty if execution-info feature is NOT enabled)
    pub cancel_async_message_execution: Vec<(Address, Result<Amount, String>)>,
    /// Auto sell roll execution (empty if execution-info feature is NOT enabled)
    pub auto_sell_execution: Vec<(Address, Amount)>,
}

/// structure describing the output of a read only execution
#[derive(Debug, Clone)]
pub struct ReadOnlyExecutionOutput {
    /// Output of a single execution
    pub out: ExecutionOutput,
    /// Gas cost for this execution, with needed adjustments
    pub gas_cost: u64,
    /// Returned value from the module call
    pub call_result: Vec<u8>,
}

/// structure describing different types of read-only execution request
#[derive(Debug, Clone)]
pub struct ReadOnlyExecutionRequest {
    /// Maximum gas to spend in the execution.
    pub max_gas: u64,
    /// Call stack to simulate, older caller first
    pub call_stack: Vec<ExecutionStackElement>,
    /// Target of the request
    pub target: ReadOnlyExecutionTarget,
    /// Coins transferred to the target address during the call
    pub coins: Option<Amount>,
    /// Fee
    pub fee: Option<Amount>,
}

/// structure describing different possible targets of a read-only execution request
#[derive(Debug, Clone)]
pub enum ReadOnlyExecutionTarget {
    /// Execute the main function of a bytecode
    BytecodeExecution(Vec<u8>),

    /// Execute a function call
    FunctionCall {
        /// Target address
        target_addr: Address,
        /// Target function
        target_func: String,
        /// Parameter to pass to the target function
        parameter: Vec<u8>,
    },
}

/// structure describing a read-only call
#[derive(Debug, Clone)]
pub struct ReadOnlyCallRequest {
    /// Maximum gas to spend in the execution.
    pub max_gas: u64,
    /// Call stack to simulate, older caller first. Target should be last.
    pub call_stack: Vec<ExecutionStackElement>,
    /// Target address
    pub target_addr: Address,
    /// Target function
    pub target_func: String,
    /// Parameter to pass to the target function
    pub parameter: String,
    /// execution start state
    ///
    /// Whether to start execution from final or active state
    pub is_final: bool,
}

/// Structure describing an element of the execution stack.
/// Every time a function is called from bytecode,
/// a new `ExecutionStackElement` is pushed at the top of the execution stack
/// to represent the local execution context of the called function,
/// instead of the caller's which should lie just below in the stack.
#[derive(Debug, Clone)]
pub struct ExecutionStackElement {
    /// Called address
    pub address: Address,
    /// Coins transferred to the target address during the call
    pub coins: Amount,
    /// List of addresses owned by the current call, and on which the current call has write access.
    /// This list should contain `ExecutionStackElement::address` in the sense that an address should have write access to itself.
    /// This list should also contain all addresses created previously during the call
    /// to allow write access on newly created addresses in order to set them up,
    /// but only within the scope of the current stack element.
    /// That way, only the current scope and neither its caller not the functions it calls gain this write access,
    /// which is important for security.
    /// Note that we use a vector instead of a pre-hashed set to ensure order determinism,
    /// the performance hit of linear search remains minimal because `owned_addresses` will always contain very few elements.
    pub owned_addresses: Vec<Address>,
    /// Datastore (key value store) for `ExecuteSC` Operation
    pub operation_datastore: Option<Datastore>,
}
