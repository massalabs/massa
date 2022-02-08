use crate::sce_ledger::{FinalLedger, SCELedger, SCELedgerChanges, SCELedgerStep};
use crate::BootstrapExecutionState;
use massa_models::api::SCELedgerInfo;
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::output_event::{SCOutputEvent, SCOutputEventId};
use massa_models::prehash::{Map, PreHashed, Set};
/// Define types used while executing block bytecodes
use massa_models::{Address, Amount, Block, BlockId, OperationId, Slot};
use massa_sc_runtime::Bytecode;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use std::cmp;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::hash::Hash;
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

    /// events produced during this step
    pub events: EventStore,
}

/// Keep all events you need with some useful indexes
#[derive(Default, Debug, Clone)]
pub(crate) struct EventStore {
    /// maps ids to events
    id_to_event: Map<SCOutputEventId, SCOutputEvent>,

    /// maps slot to a set of event ids
    slot_to_id: HashMap<Slot, Set<SCOutputEventId>>,

    /// maps initial caller to a set of event ids
    caller_to_id: Map<Address, Set<SCOutputEventId>>,

    /// maps direct event producer to a set of event ids
    smart_contract_to_id: Map<Address, Set<SCOutputEventId>>,

    /// maps operation id to a set of event ids
    operation_id_to_event_id: Map<OperationId, Set<SCOutputEventId>>,
}

impl EventStore {
    /// add event to the store and all its indexes
    pub fn insert(&mut self, id: SCOutputEventId, event: SCOutputEvent) {
        if let Entry::Vacant(entry) = self.id_to_event.entry(id) {
            self.slot_to_id
                .entry(event.context.slot)
                .or_insert_with(Set::<SCOutputEventId>::default)
                .insert(id);
            if let Some(&caller) = event.context.call_stack.front() {
                self.caller_to_id
                    .entry(caller)
                    .or_insert_with(Set::<SCOutputEventId>::default)
                    .insert(id);
            }
            if let Some(&sc) = event.context.call_stack.back() {
                self.smart_contract_to_id
                    .entry(sc)
                    .or_insert_with(Set::<SCOutputEventId>::default)
                    .insert(id);
            }
            if let Some(op) = event.context.origin_operation_id {
                self.operation_id_to_event_id
                    .entry(op)
                    .or_insert_with(Set::<SCOutputEventId>::default)
                    .insert(id);
            }
            entry.insert(event);
        } else {
            // just warn or return error ?
            warn!("execution event already exist {:?}", id)
        }
    }

    /// get just the map if ids to events
    pub fn export(&self) -> Map<SCOutputEventId, SCOutputEvent> {
        self.id_to_event.clone()
    }

    /// Clears the map, removing all key-value pairs. Keeps the allocated memory for reuse.
    pub fn clear(&mut self) {
        self.id_to_event.clear();
        self.slot_to_id.clear();
        self.caller_to_id.clear();
        self.smart_contract_to_id.clear();
        self.operation_id_to_event_id.clear();
    }

    /// Prune the exess of events from the event store,
    /// While there is a slot found, pop slots and get the `event_ids`
    /// inside, remove the event from divers containers.
    ///
    /// Return directly if the event_size <= max_final_events
    pub fn prune(&mut self, max_final_events: usize) {
        let mut events_size = self.id_to_event.len();
        if events_size <= max_final_events {
            return;
        }
        let mut slots = self.slot_to_id.keys().copied().collect::<Vec<_>>();
        slots.sort_unstable_by_key(|s| cmp::Reverse(*s));
        loop {
            let slot = match slots.pop() {
                Some(slot) => slot,
                _ => return,
            };
            let event_ids = match self.slot_to_id.get(&slot) {
                Some(event_ids) => event_ids.clone(),
                _ => continue,
            };
            for event_id in event_ids.iter() {
                let event = match self.id_to_event.remove(event_id) {
                    Some(event) => event,
                    _ => continue, /* This shouldn't happen */
                };
                remove_from_hashmap(&mut self.slot_to_id, &event.context.slot, event_id);
                if let Some(caller) = event.context.call_stack.front() {
                    remove_from_map(&mut self.caller_to_id, caller, event_id);
                }
                if let Some(sc) = event.context.call_stack.back() {
                    remove_from_map(&mut self.smart_contract_to_id, sc, event_id);
                }
                if let Some(op) = event.context.origin_operation_id {
                    remove_from_map(&mut self.operation_id_to_event_id, &op, event_id);
                }
                events_size -= 1;
                if events_size <= max_final_events {
                    return;
                }
            }
        }
    }

    /// Extend an event store with another one
    pub fn extend(&mut self, other: EventStore) {
        self.id_to_event.extend(other.id_to_event);

        other
            .slot_to_id
            .iter()
            .for_each(|(slot, ids)| match self.slot_to_id.get_mut(slot) {
                Some(set) => set.extend(ids),
                None => {
                    self.slot_to_id.insert(*slot, ids.clone());
                }
            });

        other.caller_to_id.iter().for_each(|(caller, ids)| {
            match self.caller_to_id.get_mut(caller) {
                Some(set) => set.extend(ids),
                None => {
                    self.caller_to_id.insert(*caller, ids.clone());
                }
            }
        });

        other.smart_contract_to_id.iter().for_each(|(sc, ids)| {
            match self.smart_contract_to_id.get_mut(sc) {
                Some(set) => set.extend(ids),
                None => {
                    self.smart_contract_to_id.insert(*sc, ids.clone());
                }
            }
        });

        other.operation_id_to_event_id.iter().for_each(|(op, ids)| {
            match self.operation_id_to_event_id.get_mut(op) {
                Some(set) => set.extend(ids),
                None => {
                    self.operation_id_to_event_id.insert(*op, ids.clone());
                }
            }
        })
    }

    /// get vec of event for given slot range (start included, end excluded)
    /// Get events optionnally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    pub fn get_filtered_sc_output_event(
        &self,
        start: Slot,
        end: Slot,
        emitter_address: Option<Address>,
        original_caller_address: Option<Address>,
        original_operation_id: Option<OperationId>,
    ) -> Vec<SCOutputEvent> {
        let empty = Set::<SCOutputEventId>::default();
        self.slot_to_id
            .iter()
            // filter on slots
            .filter_map(|(slot, ids)| {
                if slot >= &start && slot < &end {
                    Some(ids)
                } else {
                    None
                }
            })
            .flatten()
            // filter on original caller
            .chain(if let Some(addr) = original_caller_address {
                match self.caller_to_id.get(&addr) {
                    Some(it) => it.iter(),
                    None => empty.iter(),
                }
            } else {
                empty.iter()
            })
            // filter on emitter
            .chain(if let Some(addr) = emitter_address {
                match self.smart_contract_to_id.get(&addr) {
                    Some(it) => it.iter(),
                    None => empty.iter(),
                }
            } else {
                empty.iter()
            })
            // filter on operation id
            .chain(if let Some(op) = original_operation_id {
                match self.operation_id_to_event_id.get(&op) {
                    Some(it) => it.iter(),
                    None => empty.iter(),
                }
            } else {
                empty.iter()
            })
            .filter_map(|id| self.id_to_event.get(id))
            .cloned()
            .collect()
    }
}

#[derive(Clone)]
pub struct StackElement {
    /// called address
    pub address: Address,
    /// coins transferred to the target address during a call,
    pub coins: Amount,
    /// list of addresses created so far during excution,
    pub owned_addresses: Vec<Address>,
}

#[derive(Clone)]
/// Stateful context, providing a context during the execution of a module
pub(crate) struct ExecutionContext {
    /// final and active ledger at the current step
    pub ledger_step: SCELedgerStep,

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
    pub stack: Vec<StackElement>,

    /// True if it's a read-only context
    pub read_only: bool,

    /// geerated events during this execution, with multiple indexes
    pub events: EventStore,

    /// Unsafe RNG state
    pub unsafe_rng: Xoshiro256PlusPlus,

    /// origin operation id
    pub origin_operation_id: Option<OperationId>,
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
            stack: Default::default(),
            gas_price: Default::default(),
            slot: Slot::new(0, 0),
            opt_block_id: Default::default(),
            opt_block_creator_addr: Default::default(),
            created_addr_index: Default::default(),
            read_only: Default::default(),
            created_event_index: Default::default(),
            events: Default::default(),
            unsafe_rng: Xoshiro256PlusPlus::from_seed([0u8; 32]),
            origin_operation_id: Default::default(),
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
    /// Get events optionnally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    GetSCOutputEvents {
        start: Option<Slot>,
        end: Option<Slot>,
        emitter_address: Option<Address>,
        original_caller_address: Option<Address>,
        original_operation_id: Option<OperationId>,
        response_tx: oneshot::Sender<Vec<SCOutputEvent>>,
    },
    /// Get ledger entry for address
    GetSCELedgerForAddresses {
        response_tx: oneshot::Sender<Map<Address, SCELedgerInfo>>,
        addresses: Vec<Address>,
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

#[inline]
/// Remove a given event_id from a `Set<SCOutputEventId>`
/// The Set is stored into a map `ctnr` at a `key` address. If
/// the Set resulted from the operation is empty, remove the entry
/// from the `ctnr`
///
/// Used in `prune()`
fn remove_from_map<T: Eq + Hash + PreHashed>(
    ctnr: &mut Map<T, Set<SCOutputEventId>>,
    key: &T,
    evt_id: &SCOutputEventId,
) {
    match ctnr.get_mut(key) {
        Some(ele) => {
            ele.remove(evt_id);
            if ele.is_empty() {
                ctnr.remove(key);
            }
        }
        _ => {
            ctnr.remove(key);
        }
    }
}

#[inline]
/// Remove a given event_id from a `Set<SCOutputEventId>`
/// The Set is stored into a Hashmap `ctnr` at a `key` address. If
/// the Set resulted from the operation is empty, remove the entry
/// from the `ctnr`
///
/// Used in `prune()`
fn remove_from_hashmap<T: Eq + Hash>(
    ctnr: &mut HashMap<T, Set<SCOutputEventId>>,
    key: &T,
    evt_id: &SCOutputEventId,
) {
    match ctnr.get_mut(key) {
        Some(ele) => {
            ele.remove(evt_id);
            if ele.is_empty() {
                ctnr.remove(key);
            }
        }
        _ => {
            ctnr.remove(key);
        }
    }
}
