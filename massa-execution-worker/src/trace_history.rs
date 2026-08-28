use massa_execution_exports::types_trace_info::AbiTrace;
use massa_execution_exports::types_trace_info::{SlotAbiCallStack, Transfer};
use massa_models::{operation::OperationId, slot::Slot};
use schnellru::{ByLength, LruMap};
use std::collections::HashSet;

/// Execution traces history
pub struct TraceHistory {
    /// Maximum number of slots retained in each slot-level cache
    max_slot_size_cache: u32,
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
            max_slot_size_cache,
            trace_per_slot: LruMap::new(ByLength::new(max_slot_size_cache)),
            op_per_slot: LruMap::new(ByLength::new(
                max_slot_size_cache.saturating_mul(op_per_slot),
            )),
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

    /// Remove traces, transfers and reverse-index entries for a slot.
    fn remove_slot(&mut self, slot: &Slot) {
        let mut operation_ids = HashSet::new();

        if let Some(traces) = self.trace_per_slot.remove(slot) {
            operation_ids.extend(traces.operation_call_stacks.into_keys());
        }
        if let Some(transfers) = self.transfer_per_slot.remove(slot) {
            operation_ids.extend(transfers.into_iter().map(|transfer| transfer.op_id));
        }

        for operation_id in operation_ids {
            if self.op_per_slot.peek(&operation_id) == Some(slot) {
                self.op_per_slot.remove(&operation_id);
            }
        }
    }

    /// Remove a slot and every later slot from the history.
    pub(crate) fn truncate_from(&mut self, slot: &Slot) {
        let slots_to_remove: HashSet<_> = self
            .trace_per_slot
            .iter()
            .map(|(cached_slot, _)| *cached_slot)
            .chain(
                self.transfer_per_slot
                    .iter()
                    .map(|(cached_slot, _)| *cached_slot),
            )
            .filter(|cached_slot| cached_slot >= slot)
            .collect();

        for slot_to_remove in slots_to_remove {
            self.remove_slot(&slot_to_remove);
        }
    }

    /// Save execution traces and transfers as one replacement for a slot.
    pub(crate) fn save_for_slot(
        &mut self,
        slot: Slot,
        traces: SlotAbiCallStack,
        transfers: Vec<Transfer>,
    ) {
        self.remove_slot(&slot);

        if self.max_slot_size_cache == 0 {
            return;
        }

        if self.trace_per_slot.len() >= self.max_slot_size_cache as usize {
            if let Some((oldest_slot, _)) = self.trace_per_slot.peek_oldest() {
                let oldest_slot = *oldest_slot;
                self.remove_slot(&oldest_slot);
            }
        }

        for op_id in traces.operation_call_stacks.keys() {
            self.op_per_slot.insert(*op_id, slot);
        }
        for transfer in &transfers {
            self.op_per_slot.insert(transfer.op_id, slot);
        }

        self.trace_per_slot.insert(slot, traces);
        self.transfer_per_slot.insert(slot, transfers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_hash::Hash;
    use massa_models::{address::Address, amount::Amount, prehash::PreHashMap, secure_share::Id};
    use std::{collections::HashMap, str::FromStr};

    fn operation_id(seed: u8) -> OperationId {
        OperationId::new(Hash::compute_from(&[seed]))
    }

    fn traces(slot: Slot, operation_id: OperationId) -> SlotAbiCallStack {
        let mut operation_call_stacks = PreHashMap::default();
        operation_call_stacks.insert(operation_id, Vec::new());
        SlotAbiCallStack {
            slot,
            asc_call_stacks: Vec::new(),
            deferred_call_stacks: HashMap::new(),
            operation_call_stacks,
        }
    }

    fn transfer(operation_id: OperationId) -> Transfer {
        let address =
            Address::from_str("AU12cx6BJHSrBPPSE86E6LYgYS44dvXoHW77cdPbTT8H41wm6xGN5").unwrap();
        Transfer {
            from: address,
            to: address,
            amount: Amount::zero(),
            effective_received_amount: Amount::zero(),
            op_id: operation_id,
            succeed: true,
            fee: Amount::zero(),
        }
    }

    #[test]
    fn replacing_slot_removes_old_reverse_indexes() {
        let mut history = TraceHistory::new(2, 2);
        let slot = Slot::new(1, 0);
        let old_operation = operation_id(1);
        let new_operation = operation_id(2);

        history.save_for_slot(
            slot,
            traces(slot, old_operation),
            vec![transfer(old_operation)],
        );
        history.save_for_slot(
            slot,
            traces(slot, new_operation),
            vec![transfer(new_operation)],
        );

        assert!(history.fetch_traces_for_op(&old_operation).is_none());
        assert!(history.fetch_transfer_for_op(&old_operation).is_none());
        assert!(history.fetch_traces_for_op(&new_operation).is_some());
        assert_eq!(
            history
                .fetch_transfer_for_op(&new_operation)
                .map(|transfer| transfer.op_id),
            Some(new_operation)
        );
    }

    #[test]
    fn truncating_history_removes_rolled_back_slots() {
        let mut history = TraceHistory::new(3, 2);
        let retained_slot = Slot::new(1, 0);
        let rolled_back_slot = Slot::new(1, 1);
        let retained_operation = operation_id(1);
        let rolled_back_operation = operation_id(2);

        history.save_for_slot(
            retained_slot,
            traces(retained_slot, retained_operation),
            vec![transfer(retained_operation)],
        );
        history.save_for_slot(
            rolled_back_slot,
            traces(rolled_back_slot, rolled_back_operation),
            vec![transfer(rolled_back_operation)],
        );

        history.truncate_from(&rolled_back_slot);

        assert!(history.fetch_traces_for_slot(&retained_slot).is_some());
        assert!(history.fetch_transfers_for_slot(&retained_slot).is_some());
        assert!(history.fetch_traces_for_op(&retained_operation).is_some());
        assert!(history.fetch_traces_for_slot(&rolled_back_slot).is_none());
        assert!(history
            .fetch_transfers_for_slot(&rolled_back_slot)
            .is_none());
        assert!(history
            .fetch_traces_for_op(&rolled_back_operation)
            .is_none());
        assert!(history
            .fetch_transfer_for_op(&rolled_back_operation)
            .is_none());
    }

    #[test]
    fn evicting_slot_removes_reverse_indexes() {
        let mut history = TraceHistory::new(2, 2);
        let first_slot = Slot::new(1, 0);
        let second_slot = Slot::new(1, 1);
        let third_slot = Slot::new(2, 0);
        let first_operation = operation_id(1);
        let second_operation = operation_id(2);
        let third_operation = operation_id(3);

        for (slot, operation) in [
            (first_slot, first_operation),
            (second_slot, second_operation),
            (third_slot, third_operation),
        ] {
            history.save_for_slot(slot, traces(slot, operation), vec![transfer(operation)]);
        }

        assert!(history.fetch_traces_for_slot(&first_slot).is_none());
        assert!(history.fetch_transfers_for_slot(&first_slot).is_none());
        assert!(history.fetch_traces_for_op(&first_operation).is_none());
        assert!(history.fetch_transfer_for_op(&first_operation).is_none());
        assert!(history.fetch_traces_for_op(&second_operation).is_some());
        assert!(history.fetch_traces_for_op(&third_operation).is_some());
    }

    /// Replacing a slot must not unindex an operation that another slot has since recorded,
    /// otherwise re-executing a slot would hide an operation executed elsewhere.
    #[test]
    fn replacing_slot_keeps_operations_indexed_in_another_slot() {
        let mut history = TraceHistory::new(3, 2);
        let replaced_slot = Slot::new(1, 0);
        let other_slot = Slot::new(1, 1);
        let moved_operation = operation_id(1);
        let new_operation = operation_id(2);

        history.save_for_slot(
            replaced_slot,
            traces(replaced_slot, moved_operation),
            vec![transfer(moved_operation)],
        );
        history.save_for_slot(
            other_slot,
            traces(other_slot, moved_operation),
            vec![transfer(moved_operation)],
        );
        // Re-execute the first slot with another operation.
        history.save_for_slot(
            replaced_slot,
            traces(replaced_slot, new_operation),
            vec![transfer(new_operation)],
        );

        assert_eq!(
            history.op_per_slot.peek(&moved_operation),
            Some(&other_slot)
        );
        assert!(history.fetch_traces_for_op(&moved_operation).is_some());
        assert_eq!(
            history
                .fetch_transfer_for_op(&moved_operation)
                .map(|transfer| transfer.op_id),
            Some(moved_operation)
        );
        assert!(history.fetch_traces_for_op(&new_operation).is_some());
    }

    /// The combined fetch must return exactly the same data as the two separate fetches, so
    /// that switching callers to it changes only the read atomicity, not the returned data.
    #[test]
    fn fetch_slot_traces_and_transfers_matches_separate_fetches() {
        let mut history = TraceHistory::new(2, 2);
        let slot = Slot::new(3, 1);
        let operation = operation_id(1);

        history.save_for_slot(slot, traces(slot, operation), vec![transfer(operation)]);

        let (slot_traces, slot_transfers) = history.fetch_slot_traces_and_transfers(&slot);

        // Both datasets are present and come from the requested slot.
        assert_eq!(slot_traces.as_ref().map(|t| t.slot), Some(slot));
        assert_eq!(slot_transfers.as_ref().map(|t| t[0].op_id), Some(operation));

        // Equivalent to reading each dataset separately.
        assert_eq!(
            slot_traces.map(|t| t.slot),
            history.fetch_traces_for_slot(&slot).map(|t| t.slot)
        );
        assert_eq!(
            slot_transfers.map(|t| t.len()),
            history.fetch_transfers_for_slot(&slot).map(|t| t.len())
        );
    }

    /// A slot with no recorded traces/transfers yields `(None, None)`.
    #[test]
    fn fetch_slot_traces_and_transfers_missing_slot() {
        let history = TraceHistory::new(2, 2);
        let (slot_traces, slot_transfers) =
            history.fetch_slot_traces_and_transfers(&Slot::new(7, 0));
        assert!(slot_traces.is_none());
        assert!(slot_transfers.is_none());
    }
}
