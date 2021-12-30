use crate::sce_ledger::{FinalLedger, SCELedger, SCELedgerChanges, SCELedgerStep};
use massa_models::execution::ExecuteReadOnlyResponse;
/// Define types used while executing block bytecodes
use massa_models::{Address, Amount};
use massa_models::{Block, BlockId, Slot};
use std::sync::{Condvar, Mutex};
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::oneshot;

pub(crate) type StepHistory = VecDeque<StepHistoryItem>;
pub type Bytecode = Vec<u8>;

/// A StepHistory item representing the consequences of a given execution step
#[derive(Debug, Clone)]
pub(crate) struct StepHistoryItem {
    // step slot
    pub slot: Slot,

    // optional block ID (or miss if None) at that slot
    pub opt_block_id: Option<BlockId>,

    // list of SCE ledger changes caused by this execution step
    pub ledger_changes: SCELedgerChanges,
}

#[derive(Clone)]
/// Stateful context, providing an execution context to massa between module executions
pub(crate) struct ExecutionContext {
    pub ledger_step: SCELedgerStep,
    pub max_gas: u64,
    pub coins: Amount,
    pub gas_price: Amount,
    pub slot: Slot,
    pub opt_block_id: Option<BlockId>,
    pub opt_block_creator_addr: Option<Address>,
    pub call_stack: VecDeque<Address>,
}

#[derive(Clone)]
pub(crate) struct ExecutionStep {
    pub slot: Slot,
    // TODO add pos_seed for RNG seeding
    // TODO add pos_draws to list the draws (block and endorsement creators) for that slot
    pub block: Option<(BlockId, Block)>, // None if miss
}

impl ExecutionContext {
    pub fn new(ledger: SCELedger, ledger_at_slot: Slot) -> Self {
        let final_ledger_slot = FinalLedger {
            ledger,
            slot: ledger_at_slot,
        };
        ExecutionContext {
            ledger_step: SCELedgerStep {
                final_ledger_slot,
                cumulative_history_changes: Default::default(),
                caused_changes: Default::default(),
            },
            max_gas: Default::default(),
            coins: Default::default(),
            gas_price: Default::default(),
            slot: Slot::new(0, 0),
            opt_block_id: Default::default(),
            opt_block_creator_addr: Default::default(),
            call_stack: Default::default(),
        }
    }
}

impl From<StepHistory> for SCELedgerChanges {
    fn from(step: StepHistory) -> Self {
        let mut ret = SCELedgerChanges::default();
        step.iter()
            .for_each(|StepHistoryItem { ledger_changes, .. }| {
                ret.apply_changes(ledger_changes);
            });
        ret
    }
}

// Thread vm types:

/// execution request
pub(crate) enum ExecutionRequest {
    /// Runs a final step
    RunFinalStep(ExecutionStep),
    /// Runs an active step
    RunActiveStep(ExecutionStep),
    /// Resets the VM to its final state
    /// Run code in read-only mode
    RunReadOnly {
        /// The slot at which the execution will occur.
        slot: Slot,
        /// Maximum gas spend in execution.
        max_gas: u64,
        /// The simulated price of gas for the read-only execution.
        simulated_gas_price: Amount,
        /// The code to execute.
        bytecode: Vec<u8>,
        /// The channel used to send the result of execution.
        result_sender: oneshot::Sender<ExecuteReadOnlyResponse>,
        /// The address, or a default random one if none is provided,
        /// which will simulate the sender of the operation.
        address: Option<Address>,
    },
    ResetToFinalState,
    /// Shutdown state, set by the worker to signal shutdown to the VM thread.
    Shutdown,
}

pub(crate) type ExecutionQueue = Arc<(Mutex<VecDeque<ExecutionRequest>>, Condvar)>;
