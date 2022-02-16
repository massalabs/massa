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

pub(crate) struct ExecutionContext {
    // speculative ledger
    speculative_ledger: SpeculativeLedger,

    /// max gas for this execution
    pub max_gas: u64,

    /// gas price of the execution
    pub gas_price: Amount,

    /// slot at which the execution happens
    pub slot: Slot,

    /// counter of newly created addresses so far during this execution
    pub created_addr_index: u64,

    /// counter of newly created events so far during this execution
    pub created_event_index: u64,

    /// block ID, if one is present at this slot
    pub opt_block_id: Option<BlockId>,

    /// block creator addr, if there is a block at this slot
    pub opt_block_creator_addr: Option<Address>,

    /// address call stack, most recent is at the back
    pub stack: Vec<ExecutionStackElement>,

    /// True if it's a read-only context
    pub read_only: bool,

    /// geerated events during this execution, with multiple indexes
    pub events: EventStore,

    /// Unsafe RNG state
    pub unsafe_rng: Xoshiro256PlusPlus,

    /// origin operation id
    pub origin_operation_id: Option<OperationId>,
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
    /// The slot at which the execution will occur.
    slot: Slot,
    /// Maximum gas to spend in the execution.
    max_gas: u64,
    /// The simulated price of gas for the read-only execution.
    simulated_gas_price: Amount,
    /// The code to execute.
    bytecode: Vec<u8>,
    /// Call stack to simulate
    call_stack: Vec<Address>,
    /// The channel used to send the result of the execution.
    result_sender: std::sync::mpsc::Sender<Result<ExecutionOutput, ExecutionError>>,
}
