//! Speculative async call registry.

use crate::active_history::ActiveHistory;
use massa_asc::{AsyncCall, AsyncRegistryChanges, AsyncSlotCallsMap, CallStatus};
use massa_execution_exports::ExecutionError;
use massa_final_state::FinalStateController;
use massa_models::{address::Address, amount::Amount, asc_call_id::AsyncCallId, slot::Slot};
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
        self.current_changes.set_call(id, call);
    }

    /// Consumes all calls targeting a given slot, deletes them
    pub fn take_calls_targeting_slot(&mut self, slot: Slot) -> AsyncSlotCallsMap {
        // add calls coming from the final state
        let mut slot_calls_map: AsyncSlotCallsMap = self
            .final_state
            .read()
            .get_asc_registry()
            .get_slot_calls(slot);

        // traverse history to add extra calls and apply changes to others
        for hist_item in self.active_history.read().0.iter() {
            slot_calls_map.apply_changes(&hist_item.state_changes.async_call_registry_changes);
        }

        // apply current changes
        slot_calls_map.apply_changes(&self.current_changes);

        // add call deletion to current changes
        for id in slot_calls_map.calls.keys() {
            self.current_changes.delete_call(slot, id);
        }

        slot_calls_map
    }

    pub fn get_call(&self, id: &AsyncCallId) -> Option<AsyncCall> {
        let slot: Slot = id.get_slot();

        // check from latest to earliest changes

        // check in current changes
        if let Some(v) = self.current_changes.get_call(&slot, id) {
            return Some(v.clone());
        }

        // check history from the most recent to the oldest item
        {
            let history = self.active_history.read();
            for history_item in history.0.iter().rev() {
                if let Some(v) = history_item
                    .state_changes
                    .async_call_registry_changes
                    .get_call(&slot, id)
                {
                    return Some(v.clone());
                }
            }
        }

        // check final state
        {
            let final_state = self.final_state.read();
            if let Some(v) = final_state.get_asc_registry().get_call(&slot, id) {
                return Some(v.clone());
            }
        }

        None
    }

    /// Cancel a call
    /// Returns the sender address and the amount of coins to reimburse them
    pub fn cancel_call(&mut self, id: &AsyncCallId) -> Result<(Address, Amount), ExecutionError> {
        // get call, fail if it does not exist
        let Some(mut call) = self.get_call(id) else {
            return Err(ExecutionError::AscError("Call ID does not exist.".into()));
        };

        // check if the call is already cancelled
        if call.cancelled {
            return Err(ExecutionError::AscError(
                "Call ID is already cancelled.".into(),
            ));
        }

        // set call as cancelled
        call.cancelled = true;

        // we need to reimburse coins to the sender
        let res = (call.sender_address, call.coins);

        // Add a cancellation to the current changes
        self.current_changes.set_call(id.clone(), call);

        Ok(res)
    }
}
