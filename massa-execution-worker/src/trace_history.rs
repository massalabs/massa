use massa_execution_exports::types_trace_info::AbiTrace;
use massa_execution_exports::types_trace_info::{SlotAbiCallStack, Transfer};
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
            transfer_per_slot: LruMap::new(ByLength::new(max_slot_size_cache)),
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

    /// Fetch both the ABI call stack and the direct transfers for a given slot in a single
    /// borrow, so both are read from the same snapshot of the trace history (i.e. the same
    /// slot execution). Prefer this over calling `fetch_traces_for_slot` and
    /// `fetch_transfers_for_slot` separately when both are needed.
    pub(crate) fn fetch_slot_traces_and_transfers(
        &self,
        slot: &Slot,
    ) -> (Option<SlotAbiCallStack>, Option<Vec<Transfer>>) {
        (
            self.trace_per_slot.peek(slot).cloned(),
            self.transfer_per_slot.peek(slot).cloned(),
        )
    }

    /// Fetch transfers for a given operation id
    pub(crate) fn fetch_transfer_for_op(&self, op_id: &OperationId) -> Option<Transfer> {
        self.op_per_slot
            .peek(op_id)
            .and_then(|slot| self.transfer_per_slot.peek(slot).cloned())
            .map(|transfers| {
                transfers
                    .into_iter()
                    .find(|transfer| transfer.op_id == *op_id)
            })
            .flatten()
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
        for transfer in transfers.iter() {
            self.op_per_slot.insert(transfer.op_id, slot);
        }
        self.transfer_per_slot.insert(slot, transfers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_models::{address::Address, amount::Amount, prehash::PreHashMap};
    use std::collections::HashMap;
    use std::str::FromStr;

    fn sample_slot_abi_call_stack(slot: Slot) -> SlotAbiCallStack {
        SlotAbiCallStack {
            slot,
            asc_call_stacks: Vec::new(),
            deferred_call_stacks: HashMap::new(),
            operation_call_stacks: PreHashMap::default(),
        }
    }

    fn sample_transfer(op_id: OperationId) -> Transfer {
        let addr =
            Address::from_str("AU1LQrXPJ3DVL8SFRqACk31E9MVxBcmCATFiRdpEmgztGxWAx48D").unwrap();
        Transfer {
            from: addr,
            to: addr,
            amount: Amount::from_raw(42),
            effective_received_amount: Amount::from_raw(42),
            op_id,
            succeed: true,
            fee: Amount::from_raw(0),
        }
    }

    /// The combined fetch must return exactly the same data as the two separate fetches, so
    /// that switching callers to it changes only the read atomicity, not the returned data.
    #[test]
    fn fetch_slot_traces_and_transfers_matches_separate_fetches() {
        let mut history = TraceHistory::new(16, 4);
        let slot = Slot::new(3, 1);
        let op_id =
            OperationId::from_str("O1q4CBcuYo8YANEV34W4JRWVHrzcYns19VJfyAB7jT4qfitAnMC").unwrap();

        history.save_traces_for_slot(slot, sample_slot_abi_call_stack(slot));
        history.save_transfers_for_slot(slot, vec![sample_transfer(op_id)]);

        let (traces, transfers) = history.fetch_slot_traces_and_transfers(&slot);

        // Both datasets are present and come from the requested slot.
        assert_eq!(traces.as_ref().map(|t| t.slot), Some(slot));
        assert_eq!(
            transfers.as_ref().map(|t| t.len()),
            history.fetch_transfers_for_slot(&slot).map(|t| t.len())
        );
        assert_eq!(transfers.as_ref().unwrap()[0].op_id, op_id);

        // Equivalent to reading each dataset separately.
        assert_eq!(
            traces.map(|t| t.slot),
            history.fetch_traces_for_slot(&slot).map(|t| t.slot)
        );
    }

    /// A slot with no recorded traces/transfers yields `(None, None)`.
    #[test]
    fn fetch_slot_traces_and_transfers_missing_slot() {
        let history = TraceHistory::new(16, 4);
        let (traces, transfers) = history.fetch_slot_traces_and_transfers(&Slot::new(7, 0));
        assert!(traces.is_none());
        assert!(transfers.is_none());
    }
}
