//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Speculative list of previously executed operations, to prevent reuse.

use crate::active_history::{ActiveHistory, HistorySearchResult};
use massa_executed_ops::ExecutedOpsChanges;
use massa_final_state::FinalState;
use massa_models::{operation::OperationId, slot::Slot};
use parking_lot::RwLock;
use std::sync::Arc;

/// Speculative state of executed operations
pub(crate) struct SpeculativeExecutedOps {
    /// Thread-safe shared access to the final state. For reading only.
    final_state: Arc<RwLock<FinalState>>,

    /// History of the outputs of recently executed slots.
    /// Slots should be consecutive, newest at the back.
    active_history: Arc<RwLock<ActiveHistory>>,

    /// executed operations: maps the operation ID to its validity slot end - included
    executed_ops: ExecutedOpsChanges,
}

impl SpeculativeExecutedOps {
    /// Creates a new `SpeculativeExecutedOps`
    ///
    /// # Arguments
    /// * `final_state`: thread-safe shared access the the final state
    /// * `active_history`: thread-safe shared access the speculative execution history
    pub(crate)  fn new(
        final_state: Arc<RwLock<FinalState>>,
        active_history: Arc<RwLock<ActiveHistory>>,
    ) -> Self {
        SpeculativeExecutedOps {
            final_state,
            active_history,
            executed_ops: Default::default(),
        }
    }

    /// Returns the set of operation IDs caused to the `SpeculativeExecutedOps` since its creation,
    /// and resets their local value to nothing
    pub(crate)  fn take(&mut self) -> ExecutedOpsChanges {
        std::mem::take(&mut self.executed_ops)
    }

    /// Takes a snapshot (clone) of the changes caused to the `SpeculativeExecutedOps` since its creation
    pub(crate)  fn get_snapshot(&self) -> ExecutedOpsChanges {
        self.executed_ops.clone()
    }

    /// Resets the `SpeculativeRollState` to a snapshot (see `get_snapshot` method)
    pub(crate)  fn reset_to_snapshot(&mut self, snapshot: ExecutedOpsChanges) {
        self.executed_ops = snapshot;
    }

    /// Checks if an operation was executed previously
    pub(crate)  fn is_op_executed(&self, op_id: &OperationId) -> bool {
        // check in the curent changes
        if self.executed_ops.contains_key(op_id) {
            return true;
        }

        // check in the active history, backwards
        match self.active_history.read().fetch_executed_op(op_id) {
            HistorySearchResult::Present(_) => {
                return true;
            }
            HistorySearchResult::Absent => {
                return false;
            }
            HistorySearchResult::NoInfo => {}
        }

        // check in the final state
        self.final_state.read().executed_ops.contains(op_id)
    }

    /// Insert an executed operation.
    /// Does not check for reuse, please use `SpeculativeExecutedOps::is_op_executed` before.
    ///
    /// # Arguments
    /// * `op_id`: operation ID
    /// * `op_exec_status` : the status of the execution of the operation.
    /// * `op_valid_until_slot`: slot until which the operation remains valid (included)
    pub(crate)  fn insert_executed_op(
        &mut self,
        op_id: OperationId,
        op_exec_status: bool,
        op_valid_until_slot: Slot,
    ) {
        self.executed_ops
            .insert(op_id, (op_exec_status, op_valid_until_slot));
    }
}
