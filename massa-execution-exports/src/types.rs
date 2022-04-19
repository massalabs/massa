// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file exports useful types used to interact with the execution worker

use crate::event_store::EventStore;
use massa_final_state::StateChanges;
use massa_models::{Address, Amount, BlockId, Slot};

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

/// structure describing different types of read-only execution request
#[derive(Debug, Clone)]
pub struct ReadOnlyExecutionRequest {
    /// Maximum gas to spend in the execution.
    pub max_gas: u64,
    /// The simulated price of gas for the read-only execution.
    pub simulated_gas_price: Amount,
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
        parameter: String,
    },
}

/// structure describing a read-only call
#[derive(Debug, Clone)]
pub struct ReadOnlyCallRequest {
    /// Maximum gas to spend in the execution.
    pub max_gas: u64,
    /// The simulated price of gas for the read-only execution.
    pub simulated_gas_price: Amount,
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
}
