use std::collections::BTreeMap;

use massa_execution_exports::ExecutionOperationTrace;
use massa_models::{block_id::BlockId, operation::OperationId, slot::Slot};

#[derive(Clone, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum TraceHistoryStatus {
    Speculative,
    Finalized,
}

/// Execution traces history
#[derive(Default)]
pub struct TraceHistory(
    pub BTreeMap<(Slot, BlockId, TraceHistoryStatus), Vec<ExecutionOperationTrace>>,
);

impl TraceHistory {
    /// Remove the oldest entries, only keep 'limit' entries in history
    pub(crate) fn prune(&mut self, limit: usize) {
        while self.0.len() > limit {
            self.0.pop_first();
        }
    }

    /// Fetch execution traces for a given slot
    pub(crate) fn fetch_traces_for_slot<'a>(
        &'a self,
        slot: &'a Slot,
    ) -> impl Iterator<
        Item = (
            (Slot, BlockId, TraceHistoryStatus),
            Vec<ExecutionOperationTrace>,
        ),
    > + 'a {
        self.0.iter().rev().filter_map(|(k, v)| {
            if k.0 == *slot {
                Some((k.clone(), v.clone()))
            } else {
                None
            }
        })
    }

    /// Fetch execution traces for a given operation id
    pub(crate) fn fetch_traces_for_operation_id<'a>(
        &'a self,
        operation_id: &'a OperationId,
    ) -> impl Iterator<
        Item = (
            (Slot, BlockId, TraceHistoryStatus),
            Vec<ExecutionOperationTrace>,
        ),
    > + 'a {
        self.0.iter().rev().map(move |(k, v)| {
            (
                k.clone(),
                v.iter()
                    .filter(|t| t.operation_id == *operation_id)
                    .cloned()
                    .collect(),
            )
        })
    }
}
