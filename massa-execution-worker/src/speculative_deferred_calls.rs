//! Speculative async call registry.

use crate::active_history::ActiveHistory;
use massa_deferred_calls::{
    registry_changes::DeferredRegistryChanges, DeferredCall, DeferredSlotCalls,
};
use massa_execution_exports::ExecutionError;
use massa_final_state::FinalStateController;
use massa_models::{
    address::Address, amount::Amount, deferred_call_id::DeferredCallId, slot::Slot,
};
use parking_lot::RwLock;
use std::sync::Arc;

pub(crate) struct SpeculativeDeferredCallRegistry {
    final_state: Arc<RwLock<dyn FinalStateController>>,
    active_history: Arc<RwLock<ActiveHistory>>,
    // current speculative registry changes
    deferred_calls_changes: DeferredRegistryChanges,
}

impl SpeculativeDeferredCallRegistry {
    /// Creates a new `SpeculativeDeferredCallRegistry`
    ///
    /// # Arguments
    pub fn new(
        final_state: Arc<RwLock<dyn FinalStateController>>,
        active_history: Arc<RwLock<ActiveHistory>>,
    ) -> Self {
        SpeculativeDeferredCallRegistry {
            final_state,
            active_history,
            deferred_calls_changes: Default::default(),
        }
    }

    /// Takes a snapshot (clone) of the message states
    pub fn get_snapshot(&self) -> DeferredRegistryChanges {
        self.deferred_calls_changes.clone()
    }

    /// Resets the `SpeculativeDeferredCallRegistry` to a snapshot (see `get_snapshot` method)
    pub fn reset_to_snapshot(&mut self, snapshot: DeferredRegistryChanges) {
        self.deferred_calls_changes = snapshot;
    }

    /// Add a new call to the list of changes of this `SpeculativeDeferredCallRegistry`
    pub fn push_new_call(&mut self, id: DeferredCallId, call: DeferredCall) {
        self.deferred_calls_changes.set_call(id, call);
    }

    pub fn get_slot_gas(&self, slot: &Slot) -> u64 {
        unimplemented!("get_slot_gas");

        // // get slot gas from current changes
        // if let Some(v) = self.current_changes.get_slot_gas(slot) {
        //     return v;
        // }

        // // check in history backwards
        // {
        //     let history = self.active_history.read();
        //     for history_item in history.0.iter().rev() {
        //         if let Some(v) = history_item
        //             .state_changes
        //             .async_call_registry_changes
        //             .get_slot_gas(slot)
        //         {
        //             return v;
        //         }
        //     }
        // }

        // // check in final state
        // return self
        //     .final_state
        //     .read()
        //     .get_asc_registry()
        //     .get_slot_gas(slot);
    }

    pub fn get_slot_base_fee(&self, slot: &Slot) -> Amount {
        // get slot base fee from current changes
        if let Some(v) = self.deferred_calls_changes.get_slot_base_fee(slot) {
            return v;
        }

        // check in history backwards
        {
            let history = self.active_history.read();
            for history_item in history.0.iter().rev() {
                if let Some(v) = history_item
                    .state_changes
                    .deferred_call_changes
                    .get_slot_base_fee(slot)
                {
                    return v;
                }
            }
        }

        // check in final state
        // todo check if that is correct
        return self
            .final_state
            .read()
            .get_deferred_call_registry()
            .get_slot_base_fee(slot);
    }

    /// Consumes and deletes the current slot, prepares a new slot in the future
    /// and returns the calls that need to be executed in the current slot
    pub fn advance_slot(
        &mut self,
        current_slot: Slot,
        async_call_max_booking_slots: u64,
        thread_count: u8,
    ) -> DeferredSlotCalls {
        // get the state of the current slot
        let mut slot_calls: DeferredSlotCalls = self
            .final_state
            .read()
            .get_deferred_call_registry()
            .get_slot_calls(current_slot);
        for hist_item in self.active_history.read().0.iter() {
            slot_calls.apply_changes(&hist_item.state_changes.deferred_call_changes);
        }
        slot_calls.apply_changes(&self.deferred_calls_changes);

        // select the slot that is newly made available and set its base fee
        let mut new_slot = current_slot
            .skip(async_call_max_booking_slots, thread_count)
            .expect("could not skip enough slots");
        todo!();
        self.deferred_calls_changes
            .set_slot_base_fee(new_slot, todo!());

        // subtract the current slot gas from the total gas
        self.deferred_calls_changes.set_total_gas(
            slot_calls
                .total_gas
                .saturating_sub(slot_calls.slot_gas.into()),
        );

        // delete the current slot
        for (id, call) in &slot_calls.slot_calls {
            self.deferred_calls_changes.delete_call(current_slot, id);
        }
        self.deferred_calls_changes.set_slot_gas(current_slot, 0);
        self.deferred_calls_changes
            .set_slot_base_fee(current_slot, Amount::zero());

        slot_calls
    }

    pub fn get_call(&self, id: &DeferredCallId) -> Option<DeferredCall> {
        let slot: Slot = id.get_slot();

        // check from latest to earliest changes

        // check in current changes
        if let Some(v) = self.deferred_calls_changes.get_call(&slot, id) {
            return Some(v.clone());
        }

        // check history from the most recent to the oldest item
        {
            let history = self.active_history.read();
            for history_item in history.0.iter().rev() {
                if let Some(v) = history_item
                    .state_changes
                    .deferred_call_changes
                    .get_call(&slot, id)
                {
                    return Some(v.clone());
                }
            }
        }

        // check final state
        {
            let final_state = self.final_state.read();
            // if let Some(v) = final_state.get_deferred_call_registry().get_call(&slot, id) {
            if let Some(v) = final_state.get_deferred_call_registry().get_call(&slot, id) {
                return Some(v.clone());
            }
        }

        None
    }

    /// Cancel a call
    /// Returns the sender address and the amount of coins to reimburse them
    pub fn cancel_call(
        &mut self,
        id: &DeferredCallId,
    ) -> Result<(Address, Amount), ExecutionError> {
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
        self.deferred_calls_changes.set_call(id.clone(), call);

        Ok(res)
    }

    // This function assumes that we have a resource with a total supply `resource_supply`.
    // Below a certain target occupancy `target_occupancy` of that resource, the overbooking penalty for using a unit of the resource is zero.
    // Above the target, the resource unit cost grows linearly with occupancy.
    // The linear fee growth is chosen so that if someone occupies all the resource, they incur a `max_penalty` cost.
    fn overbooking_fee(
        resource_supply: u128,
        target_occupancy: u128,
        current_occupancy: u128,
        resource_request: u128,
        max_penalty: Amount,
    ) -> Amount {
        // linear part of the occupancy before booking the requested amount
        let relu_occupancy_before =
            std::cmp::max(current_occupancy, target_occupancy) - target_occupancy;

        // linear part of the occupancy after booking the requested amount
        let relu_occupancy_after = std::cmp::max(
            current_occupancy.saturating_add(resource_request),
            target_occupancy,
        ) - target_occupancy;

        // denominator for the linear fee
        let denominator = resource_supply - target_occupancy;

        // compute using the raw fee and u128 to avoid u64 overflows
        let raw_max_penalty = max_penalty.to_raw() as u128;

        let raw_fee = (raw_max_penalty * relu_occupancy_after / denominator * relu_occupancy_after
            / denominator)
            .saturating_sub(
                raw_max_penalty * relu_occupancy_before / denominator * relu_occupancy_before
                    / denominator,
            );

        Amount::from_raw(std::cmp::min(raw_fee, u64::MAX as u128) as u64)
    }

    // pub fn get_slot_base_fee(&self, slot: &Slot) -> Amount {
    //     // get slot base fee from current changes
    //     if let Some(v) = self.current_changes.get_slot_base_fee(slot) {
    //         return v;
    //     }

    //     // check in history backwards
    //     {
    //         let history = self.active_history.read();
    //         for history_item in history.0.iter().rev() {
    //             if let Some(v) = history_item
    //                 .state_changes
    //                 .async_call_registry_changes
    //                 .get_slot_base_fee(slot)
    //             {
    //                 return v;
    //             }
    //         }
    //     }

    //     // check in final state
    //     return self
    //         .final_state
    //         .read()
    //         .get_asc_registry()
    //         .get_slot_base_fee(slot);
    // }

    /// Compute call fee
    pub fn compute_call_fee(
        &self,
        target_slot: Slot,
        max_gas: u64,
        thread_count: u8,
        async_call_max_booking_slots: u64,
        max_async_gas: u64,
        async_gas_target: u64,
        global_overbooking_penalty: Amount,
        slot_overbooking_penalty: Amount,
        current_slot: Slot,
    ) -> Result<Amount, ExecutionError> {
        // Check that the slot is not in the past
        if target_slot <= current_slot {
            return Err(ExecutionError::AscError(
                "Target slot is in the past.".into(),
            ));
        }

        // Check that the slot is not in the future
        if target_slot
            .slots_since(&current_slot, thread_count)
            .unwrap_or(u64::MAX)
            > async_call_max_booking_slots
        {
            // note: the current slot is not counted
            return Err(ExecutionError::AscError(
                "Target slot is too far in the future.".into(),
            ));
        }

        // Check that the gas is not too high for the target slot
        let slot_occupancy = self.get_slot_gas(&target_slot);
        if slot_occupancy.saturating_add(max_gas) > max_async_gas {
            return Err(ExecutionError::AscError(
                "Not enough gas available in the target slot.".into(),
            ));
        }

        // Integral fee
        let integral_fee = self
            .get_slot_base_fee(&target_slot)
            .saturating_mul_u64(max_gas);

        // Slot overbooking fee
        let slot_overbooking_fee = Self::overbooking_fee(
            max_async_gas as u128,
            async_gas_target as u128,
            slot_occupancy as u128,
            max_gas as u128,
            slot_overbooking_penalty,
        );

        // Global overbooking fee
        // todo check if this is correct
        let global_occupancy = self
            .deferred_calls_changes
            .get_total_gas()
            .unwrap_or_default();
        let global_overbooking_fee = Self::overbooking_fee(
            (max_async_gas as u128).saturating_mul(async_call_max_booking_slots as u128),
            (async_gas_target as u128).saturating_mul(async_call_max_booking_slots as u128),
            global_occupancy,
            max_gas as u128,
            global_overbooking_penalty,
        );

        // return the fee
        Ok(integral_fee
            .saturating_add(global_overbooking_fee)
            .saturating_add(slot_overbooking_fee))
    }

    /// Take the deferred registry slot changes
    pub(crate) fn take(&mut self) -> DeferredRegistryChanges {
        std::mem::take(&mut self.deferred_calls_changes)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_compute_call_fee() {}
}
