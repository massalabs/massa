//! Speculative async call registry.

use crate::active_history::ActiveHistory;
use massa_asc::{AsyncCall, AsyncRegistryChanges, CallStatus};
use massa_execution_exports::ExecutionError;
use massa_final_state::FinalStateController;
use massa_models::{asc_call_id::AsyncCallId, slot::Slot};
use parking_lot::RwLock;
use std::sync::Arc;

pub(crate) struct SpeculativeAsyncCallRegistry {
    final_state: Arc<RwLock<dyn FinalStateController>>,
    active_history: Arc<RwLock<ActiveHistory>>,
    // current speculative registry changes
    current_changes: AsyncRegistryChanges,
}

impl SpeculativeAsyncCallRegistry {
    /// Creates a new `SpeculativeAsyncCallRegistry`
    ///
    /// # Arguments
    pub fn new(
        final_state: Arc<RwLock<dyn FinalStateController>>,
        active_history: Arc<RwLock<ActiveHistory>>,
    ) -> Self {
        SpeculativeAsyncCallRegistry {
            final_state,
            active_history,
            current_changes: Default::default(),
        }
    }

    /// Takes a snapshot (clone) of the message states
    pub fn get_snapshot(&self) -> AsyncRegistryChanges {
        self.current_changes.clone()
    }

    /// Resets the `SpeculativeAsyncCallRegistry` to a snapshot (see `get_snapshot` method)
    pub fn reset_to_snapshot(&mut self, snapshot: AsyncRegistryChanges) {
        self.current_changes = snapshot;
    }

    /// Add a new call to the list of changes of this `SpeculativeAsyncCallRegistry`
    pub fn push_new_call(&mut self, id: AsyncCallId, call: AsyncCall) {
        self.current_changes.push_new_call(id, call);
    }

    /// Removes the next call to be executed at the given slot and returns it.
    /// Returns None if there is no call to be executed at the given slot.
    pub fn take_next_call(&mut self, slot: Slot) -> Option<(AsyncCallId, AsyncCall)> {
        // Note: calls can only be added. So we want to look from old to new and return the first one.

        let mut res = None;

        // check final state
        if res.is_none() {
            res = self
                .final_state
                .read()
                .get_asc_registry()
                .get_best_call(slot); // note: includes cancelled calls
        }

        // check active history from oldest to newest
        if res.is_none() {
            let history = self.active_history.read();
            for history_item in &history.0 {
                res = history_item
                    .state_changes
                    .async_call_registry_changes
                    .get_best_call(slot);
                if res.is_some() {
                    break;
                }
            }
        }

        // check current changes
        if res.is_none() {
            res = self.current_changes.get_best_call(slot);
        }

        // remove from current changes
        if let Some((id, _)) = res.as_ref() {
            self.current_changes.remove_call(slot, id);
        }

        res
    }

    /// Check that a call exists and is not cancelled
    pub fn call_exists(&mut self, id: AsyncCallId) -> bool {
        // Check if the call was cancelled or deleted in the current changes
        match self.current_changes.call_status(id.get_slot(), &id) {
            CallStatus::Emitted => return true,
            CallStatus::Cancelled | CallStatus::Removed => return false,
            CallStatus::Unknown => {}
        }

        // List history from most recent to oldest, and check if the call was cancelled or deleted
        {
            let history = self.active_history.read();
            for history_item in history.0.iter().rev() {
                match history_item
                    .state_changes
                    .async_call_registry_changes
                    .get_status(id)
                {
                    CallStatus::Emitted => return true,
                    CallStatus::Cancelled | CallStatus::Removed => return false,
                    CallStatus::Unknown => {}
                }
            }
        }

        // Check the final state: return true if the call is present and not cancelled
        {
            let final_state = self.final_state.read();
            match final_state.get_asc_registry().is_call_cancelled(id) {
                None => false,       // not found
                Some(true) => false, // found, cancelled
                Some(false) => true, // found, not cancelled
            }
        }
    }

    /// Cancel a call
    pub fn cancel_call(&mut self, id: AsyncCallId) -> Result<(), ExecutionError> {
        if !self.call_exists(id) {
            return Err(ExecutionError::AscError("Call ID does not exist.".into()));
        }
        // Add a cancellation to the current changes
        self.current_changes.cancel_call(id.get_slot(), id);
        Ok(())
    }
}
