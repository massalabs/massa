use massa_execution_exports::{AbiTrace, SlotAbiCallStack, Transfer};
use massa_models::{operation::OperationId, slot::Slot};
use schnellru::{ByLength, LruMap};

/// Execution traces history
pub struct TraceHistory {
    /// Execution traces history by slot
    trace_per_slot: LruMap<Slot, SlotAbiCallStack>,
    /// Transfer coins by slot
    transfer_per_slot: LruMap<Slot, Vec<Transfer>>,
    /// Execution op linked to slot
    op_per_slot: LruMap<OperationId, Slot>,
}

impl TraceHistory {
    pub fn new(max_slot_size_cache: u32, op_per_slot: u32) -> Self {
        Self {
            trace_per_slot: LruMap::new(ByLength::new(max_slot_size_cache)),
            op_per_slot: LruMap::new(ByLength::new(max_slot_size_cache * op_per_slot)),
            transfer_per_slot: LruMap::new(ByLength::new(max_slot_size_cache * op_per_slot)),
        }
    }

    /// Fetch execution traces for a given slot
    pub(crate) fn fetch_traces_for_slot(&self, slot: &Slot) -> Option<SlotAbiCallStack> {
        self.trace_per_slot.peek(slot).cloned()
    }

    /// Fetch slot for a given operation
    pub(crate) fn fetch_traces_for_op(&self, op_id: &OperationId) -> Option<Vec<AbiTrace>> {
        self.op_per_slot
            .peek(op_id)
            .and_then(|slot| {
                self.trace_per_slot
                    .peek(slot)
                    .map(|trace| trace.operation_call_stacks.get(op_id).cloned())
            })
            // .flatten()
            .flatten()
    }

    /// Fetch transfer for a given slot
    pub(crate) fn fetch_transfers_for_slot(&self, slot: &Slot) -> Option<Vec<Transfer>> {
        self.transfer_per_slot.peek(slot).cloned()
    }

    /// Save execution traces for a given slot
    pub(crate) fn save_traces_for_slot(&mut self, slot: Slot, traces: SlotAbiCallStack) {
        for (op_id, _) in traces.operation_call_stacks.iter() {
            self.op_per_slot.insert(*op_id, slot);
        }
        self.trace_per_slot.insert(slot, traces);
    }

    /// Save transfer for a given slot
    pub(crate) fn save_transfers_for_slot(&mut self, slot: Slot, transfers: Vec<Transfer>) {
        self.transfer_per_slot.insert(slot, transfers);
    }
}
