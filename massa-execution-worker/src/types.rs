use crate::event_store::EventStore;
use crate::sce_ledger::SCELedgerChanges;
use massa_ledger::LedgerChanges;
use massa_models::{Address, Amount, BlockId, Slot};
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
