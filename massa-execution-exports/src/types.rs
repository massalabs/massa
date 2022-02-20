// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::event_store::EventStore;
use massa_ledger::LedgerChanges;
use massa_models::{Address, Amount, BlockId, Slot};

/// structure describing the output of an execution
#[derive(Debug, Clone)]
pub struct ExecutionOutput {
    // slot
    pub slot: Slot,
    // optional block ID at that slot (None if miss)
    pub block_id: Option<BlockId>,
    // ledger_changes caused by the execution step
    pub ledger_changes: LedgerChanges,
    // events emitted by the execution step
    pub events: EventStore,
}

/// structure describing a read-only execution request
#[derive(Debug, Clone)]
pub struct ReadOnlyExecutionRequest {
    /// Maximum gas to spend in the execution.
    pub max_gas: u64,
    /// The simulated price of gas for the read-only execution.
    pub simulated_gas_price: Amount,
    /// The code to execute.
    pub bytecode: Vec<u8>,
    /// Call stack to simulate
    pub call_stack: Vec<ExecutionStackElement>,
}

#[derive(Debug, Clone)]
pub struct ExecutionStackElement {
    /// called address
    pub address: Address,
    /// coins transferred to the target address during a call,
    pub coins: Amount,
    /// list of addresses created so far during excution,
    pub owned_addresses: Vec<Address>,
}
