use crate::sce_ledger::SCELedgerChanges;
use crate::speculative_ledger::SpeculativeLedger;
use crate::{event_store::EventStore, ExecutionError};
use massa_ledger::LedgerChanges;
use massa_models::{Address, Amount, BlockId, OperationId, Slot};
use rand_xoshiro::Xoshiro256PlusPlus;
use std::collections::VecDeque;

/// history of active executed steps
pub(crate) type StepHistory = VecDeque<StepHistoryItem>;

/// A StepHistory item representing the consequences of a given execution step
#[derive(Debug, Clone)]
pub(crate) struct StepHistoryItem {
    // step slot
    pub slot: Slot,

    // optional block ID (or miss if None) at that slot
    pub opt_block_id: Option<BlockId>,

    // list of SCE ledger changes caused by this execution step
    pub ledger_changes: SCELedgerChanges,

    /// events produced during this step
    pub events: EventStore,
}

#[derive(Clone)]
pub struct ExecutionStackElement {
    /// called address
    pub address: Address,
    /// coins transferred to the target address during a call,
    pub coins: Amount,
    /// list of addresses created so far during excution,
    pub owned_addresses: Vec<Address>,
}

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
