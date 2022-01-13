use crate::sce_ledger::{FinalLedger, SCELedger, SCELedgerChanges, SCELedgerStep};
use crate::BootstrapExecutionState;
use massa_models::address::AddressHashSet;
use massa_models::execution::ExecuteReadOnlyResponse;
/// Define types used while executing block bytecodes
use massa_models::{Address, Amount, Block, BlockId, Slot};
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
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
    pub created_addr_index: u64,
    pub opt_block_id: Option<BlockId>,
    pub opt_block_creator_addr: Option<Address>,
    pub call_stack: VecDeque<Address>,
    pub owned_addresses: AddressHashSet,
    pub read_only: bool,
    pub unsafe_rng: Xoshiro256PlusPlus,
}

#[derive(Clone)]
pub(crate) struct ExecutionStep {
    pub slot: Slot,
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
            owned_addresses: Default::default(),
            created_addr_index: Default::default(),
            read_only: Default::default(),
            unsafe_rng: Xoshiro256PlusPlus::from_seed([0u8; 32]),
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
    #[allow(dead_code)] // TODO DISABLED TEMPORARILY #2101
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
    /// Reset to latest final state
    ResetToFinalState,
    /// Get bootstrap state
    GetBootstrapState {
        response_tx: oneshot::Sender<BootstrapExecutionState>,
    },
    /// Shutdown state, set by the worker to signal shutdown to the VM thread.
    Shutdown,
}

pub(crate) type ExecutionQueue = Arc<(Mutex<VecDeque<ExecutionRequest>>, Condvar)>;

/// Wrapping structure for an ExecutionSC and a sender
pub struct ExecutionData {
    /// Sender address
    pub sender_address: Address,
    /// Smart contract bytecode.
    pub bytecode: Bytecode,
    /// The maximum amount of gas that the execution of the contract is allowed to cost.
    pub max_gas: u64,
    /// Extra coins that are spent by consensus and are available in the execution context of the contract.
    pub coins: Amount,
    /// The price per unit of gas that the caller is willing to pay for the execution.
    pub gas_price: Amount,
}

impl TryFrom<&massa_models::Operation> for ExecutionData {
    type Error = anyhow::Error;

    fn try_from(operation: &massa_models::Operation) -> anyhow::Result<Self> {
        match &operation.content.op {
            massa_models::OperationType::ExecuteSC {
                data,
                max_gas,
                gas_price,
                coins,
            } => Ok(ExecutionData {
                bytecode: data.to_owned(),
                sender_address: Address::from_public_key(&operation.content.sender_public_key),
                max_gas: *max_gas,
                gas_price: *gas_price,
                coins: *coins,
            }),
            _ => anyhow::bail!("Conversion require an `OperationType::ExecuteSC`"),
        }
    }
}
