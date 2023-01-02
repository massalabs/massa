// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file exports useful types used to interact with the execution worker

use crate::event_store::EventStore;
use massa_final_state::StateChanges;
use massa_models::datastore::Datastore;
use massa_models::{
    address::Address, address::ExecutionAddressCycleInfo, amount::Amount, block::BlockId,
    slot::Slot,
};
use std::collections::{BTreeMap, BTreeSet};

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

/// structure describing the output of a single execution
#[derive(Debug, Clone)]
pub struct ExecutionOutput {
    /// slot
    pub slot: Slot,
    /// optional block ID at that slot (None if miss)
    pub block_id: Option<BlockId>,
    /// state changes caused by the execution step
    pub state_changes: StateChanges,
    /// events emitted by the execution step
    pub events: EventStore,
}

/// structure describing the output of a read only execution
#[derive(Debug, Clone)]
pub struct ReadOnlyExecutionOutput {
    /// Output of a single execution
    pub out: ExecutionOutput,
    /// Gas cost for this execution
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
