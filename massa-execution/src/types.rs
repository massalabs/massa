use crate::sce_ledger::{FinalLedger, SCELedger, SCELedgerChanges, SCELedgerStep};
use crate::BootstrapExecutionState;
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::output_event::{SCOutputEvent, SCOutputEventId};
use massa_models::prehash::{Map, Set};
/// Define types used while executing block bytecodes
use massa_models::{Address, Amount, Block, BlockId, Slot};
use massa_sc_runtime::Bytecode;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::{Condvar, Mutex};
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::oneshot;
use tracing::warn;

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

    pub events: EventStore,
}

#[derive(Default, Debug, Clone)]
pub(crate) struct EventStore {
    id_to_event: Map<SCOutputEventId, SCOutputEvent>,
    slot_to_id: HashMap<Slot, Set<SCOutputEventId>>,
    caller_to_id: Map<Address, Set<SCOutputEventId>>,
    smart_contract_to_id: Map<Address, Set<SCOutputEventId>>,
}

impl EventStore {
    // add event to the store and all its indexes
    pub fn insert(&mut self, id: SCOutputEventId, event: SCOutputEvent) {
        if let Entry::Vacant(entry) = self.id_to_event.entry(id) {
            match self.slot_to_id.entry(event.context.slot) {
                Entry::Occupied(mut e) => {
                    e.get_mut().insert(id);
                }
                Entry::Vacant(e) => {
                    e.insert(vec![id].into_iter().collect());
                }
            }
            if let Some(caller) = event.context.call_stack.front() {
                self.caller_to_id
                    .get_mut(caller)
                    .unwrap_or(&mut vec![].into_iter().collect()) // TODO maybe implement default for Set
                    .insert(id);
            }
            if let Some(sc) = event.context.call_stack.back() {
                self.smart_contract_to_id
                    .get_mut(sc)
                    .unwrap_or(&mut vec![].into_iter().collect()) // TODO maybe implement default for Set
                    .insert(id);
            }
            entry.insert(event);
        } else {
            // just warn or return error ?
            warn!("execution event already exist {:?}", id)
        }
    }

    pub fn export(&self) -> Map<SCOutputEventId, SCOutputEvent> {
        self.id_to_event.clone()
    }

    pub fn clear(&mut self) {
        self.id_to_event.clear();
        self.slot_to_id.clear();
        self.caller_to_id.clear();
        self.smart_contract_to_id.clear();
    }

    pub fn get_event_for_caller(&self, caller: Address) -> Vec<SCOutputEvent> {
        let default: Set<SCOutputEventId> = vec![].into_iter().collect(); // TODO maybe implement default for Set
        let ids = self.caller_to_id.get(&caller).unwrap_or(&default);
        ids.iter()
            .filter_map(|id| self.id_to_event.get(id).cloned())
            .collect()
    }

    pub fn get_event_for_sc(&self, sc: Address) -> Vec<SCOutputEvent> {
        let default: Set<SCOutputEventId> = vec![].into_iter().collect(); // TODO maybe implement default for Set
        let ids = self.smart_contract_to_id.get(&sc).unwrap_or(&default);
        ids.iter()
            .filter_map(|id| self.id_to_event.get(id).cloned())
            .collect()
    }
}

#[derive(Clone)]
/// Stateful context, providing a context during the execution of a module
pub(crate) struct ExecutionContext {
    /// final and active ledger at the current step
    pub ledger_step: SCELedgerStep,

    /// max gas for this execution
    pub max_gas: u64,

    /// coins transferred to the target address during a call, stacked
    pub coins_stack: Vec<Amount>,

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
    pub call_stack: VecDeque<Address>,

    /// list of addresses created so far during excution, stacked
    pub owned_addresses_stack: Vec<Vec<Address>>,

    /// True if it's a read-only context
    pub read_only: bool,

    /// geerated events during this execution, with multiple indexes
    pub events: EventStore,

    /// Unsafe RNG state
    pub unsafe_rng: Xoshiro256PlusPlus,
}

/// an active execution step target slot and block
#[derive(Clone)]
pub(crate) struct ExecutionStep {
    /// slot at which the execution step will happen
    pub slot: Slot,

    /// Some(BlockID, block), if a block is present at this slot, otherwise None
    pub block: Option<(BlockId, Block)>,
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
            coins_stack: Default::default(),
            gas_price: Default::default(),
            slot: Slot::new(0, 0),
            opt_block_id: Default::default(),
            opt_block_creator_addr: Default::default(),
            call_stack: Default::default(),
            owned_addresses_stack: Default::default(),
            created_addr_index: Default::default(),
            read_only: Default::default(),
            created_event_index: Default::default(),
            events: Default::default(),
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
    GetSCOutputEventBySlotRange {
        start: Slot,
        end: Slot,
        response_tx: oneshot::Sender<Vec<SCOutputEvent>>,
    },

    GetSCOutputEventByCaller {
        caller_address: Address,
        response_tx: oneshot::Sender<Vec<SCOutputEvent>>,
    },

    GetSCOutputEventBySCAddress {
        sc_address: Address,
        response_tx: oneshot::Sender<Vec<SCOutputEvent>>,
    },
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
