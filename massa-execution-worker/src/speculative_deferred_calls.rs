//! Speculative async call registry.

use crate::active_history::ActiveHistory;
use massa_deferred_calls::{
    config::DeferredCallsConfig, registry_changes::DeferredCallRegistryChanges, DeferredCall,
    DeferredSlotCalls,
};
use massa_execution_exports::ExecutionError;
use massa_final_state::FinalStateController;
use massa_models::{
    address::Address, amount::Amount, config::MAX_ASYNC_GAS, deferred_calls::DeferredCallId,
    slot::Slot,
};
use parking_lot::RwLock;
use std::{cmp::max, sync::Arc};

const TARGET_BOOKING: u128 = (MAX_ASYNC_GAS / 2) as u128;

pub(crate) struct SpeculativeDeferredCallRegistry {
    final_state: Arc<RwLock<dyn FinalStateController>>,
    active_history: Arc<RwLock<ActiveHistory>>,
    // current speculative registry changes
    deferred_calls_changes: DeferredCallRegistryChanges,
    config: DeferredCallsConfig,
}

impl SpeculativeDeferredCallRegistry {
    /// Creates a new `SpeculativeDeferredCallRegistry`
    ///
    /// # Arguments
    pub fn new(
        final_state: Arc<RwLock<dyn FinalStateController>>,
        active_history: Arc<RwLock<ActiveHistory>>,
        config: DeferredCallsConfig,
    ) -> Self {
        SpeculativeDeferredCallRegistry {
            final_state,
            active_history,
            deferred_calls_changes: Default::default(),
            config,
        }
    }

    /// Takes a snapshot (clone) of the message states
    pub fn get_snapshot(&self) -> DeferredCallRegistryChanges {
        self.deferred_calls_changes.clone()
    }

    /// Resets the `SpeculativeDeferredCallRegistry` to a snapshot (see `get_snapshot` method)
    pub fn reset_to_snapshot(&mut self, snapshot: DeferredCallRegistryChanges) {
        self.deferred_calls_changes = snapshot;
    }

    /// Add a new call to the list of changes of this `SpeculativeDeferredCallRegistry`
    pub fn push_new_call(&mut self, id: DeferredCallId, call: DeferredCall) {
        self.deferred_calls_changes.set_call(id, call);
    }

    pub fn get_total_calls_registered(&self) -> u64 {
        if let Some(v) = self.deferred_calls_changes.get_total_calls_registered() {
            return v;
        }

        {
            let history = self.active_history.read();
            for history_item in history.0.iter().rev() {
                if let Some(v) = history_item
                    .state_changes
                    .deferred_call_changes
                    .get_total_calls_registered()
                {
                    return v;
                }
            }
        }

        return self
            .final_state
            .read()
            .get_deferred_call_registry()
            .get_nb_call_registered();
    }

    pub fn get_effective_total_gas(&self) -> u128 {
        // get total gas from current changes
        if let Some(v) = self.deferred_calls_changes.get_effective_total_gas() {
            return v;
        }

        // check in history backwards
        {
            let history = self.active_history.read();
            for history_item in history.0.iter().rev() {
                if let Some(v) = history_item
                    .state_changes
                    .deferred_call_changes
                    .get_effective_total_gas()
                {
                    return v;
                }
            }
        }

        // check in final state
        return self
            .final_state
            .read()
            .get_deferred_call_registry()
            .get_total_gas();
    }

    pub fn get_effective_slot_gas(&self, slot: &Slot) -> u64 {
        // get slot gas from current changes
        if let Some(v) = self.deferred_calls_changes.get_effective_slot_gas(slot) {
            return v;
        }

        // check in history backwards
        {
            let history = self.active_history.read();
            for history_item in history.0.iter().rev() {
                if let Some(v) = history_item
                    .state_changes
                    .deferred_call_changes
                    .get_effective_slot_gas(slot)
                {
                    return v;
                }
            }
        }

        // check in final state
        return self
            .final_state
            .read()
            .get_deferred_call_registry()
            .get_slot_gas(slot);
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
        return self
            .final_state
            .read()
            .get_deferred_call_registry()
            .get_slot_base_fee(slot);
    }

    /// Consumes and deletes the current slot, prepares a new slot in the future
    /// and returns the calls that need to be executed in the current slot
    pub fn advance_slot(&mut self, current_slot: Slot) -> DeferredSlotCalls {
        // get the state of the current slot
        let mut slot_calls = self.get_calls_by_slot(current_slot);
        let total_booked_gas_before = self.get_effective_total_gas();

        let avg_booked_gas =
            total_booked_gas_before.saturating_div(self.config.max_future_slots as u128);
        // select the slot that is newly made available and set its base fee
        let new_slot = current_slot
            .skip(self.config.max_future_slots, self.config.thread_count)
            .expect("could not skip enough slots");

        let prev_slot = new_slot
            .get_prev_slot(self.config.thread_count)
            .expect("cannot get prev slot");

        let prev_slot_base_fee = self.get_slot_base_fee(&prev_slot);

        let new_slot_base_fee = match avg_booked_gas.cmp(&TARGET_BOOKING) {
            std::cmp::Ordering::Equal => prev_slot_base_fee,
            std::cmp::Ordering::Greater => {
                let gas_used_delta = avg_booked_gas.saturating_sub(TARGET_BOOKING);

                let factor = gas_used_delta as u64
                    / TARGET_BOOKING as u64
                    / self.config.base_fee_max_max_change_denominator as u64;

                max(
                    prev_slot_base_fee
                        .saturating_add(prev_slot_base_fee.saturating_mul_u64(factor)),
                    Amount::from_raw(self.config.min_gas_increment),
                )
            }
            std::cmp::Ordering::Less => {
                let gas_used_delta = TARGET_BOOKING.saturating_sub(total_booked_gas_before);

                let factor = gas_used_delta as u64
                    / TARGET_BOOKING as u64
                    / self.config.base_fee_max_max_change_denominator as u64;

                max(
                    prev_slot_base_fee
                        .saturating_sub(prev_slot_base_fee.saturating_mul_u64(factor)),
                    Amount::from_raw(self.config.min_gas_cost),
                )
            }
        };

        self.deferred_calls_changes
            .set_slot_base_fee(new_slot, new_slot_base_fee);

        // subtract the current slot gas from the total gas
        // cancelled call gas is already decremented from the effective slot gas
        let total_gas_after =
            total_booked_gas_before.saturating_sub(slot_calls.effective_slot_gas.into());
        if !total_gas_after.eq(&total_booked_gas_before) {
            self.deferred_calls_changes
                .set_effective_total_gas(total_gas_after);
        }

        slot_calls.effective_total_gas = total_gas_after;

        // delete call in the current slot
        let mut nb_call_to_execute = 0;
        for (id, call) in &slot_calls.slot_calls {
            // cancelled call is already decremented from the total calls registered
            if !call.cancelled {
                nb_call_to_execute += 1;
            }
            self.delete_call(id, current_slot);
        }

        if nb_call_to_execute > 0 {
            let total_calls_registered = self.get_total_calls_registered();
            let new_call_registered = total_calls_registered.saturating_sub(nb_call_to_execute);
            self.set_total_calls_registered(new_call_registered);
        }

        slot_calls
    }

    pub fn get_calls_by_slot(&self, slot: Slot) -> DeferredSlotCalls {
        let mut slot_calls: DeferredSlotCalls = self
            .final_state
            .read()
            .get_deferred_call_registry()
            .get_slot_calls(slot);
        for hist_item in self.active_history.read().0.iter() {
            slot_calls.apply_changes(&hist_item.state_changes.deferred_call_changes);
        }
        slot_calls.apply_changes(&self.deferred_calls_changes);
        slot_calls
    }

    pub fn get_call(&self, id: &DeferredCallId) -> Option<DeferredCall> {
        let slot = match id.get_slot() {
            Ok(slot) => slot,
            Err(_) => return None,
        };

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

    pub fn delete_call(&mut self, id: &DeferredCallId, slot: Slot) {
        self.deferred_calls_changes.delete_call(slot, id)
    }

    /// Cancel a call
    /// Returns the sender address and the amount of coins to reimburse them
    pub fn cancel_call(
        &mut self,
        id: &DeferredCallId,
    ) -> Result<(Address, Amount), ExecutionError> {
        // get call, fail if it does not exist
        let Some(mut call) = self.get_call(id) else {
            return Err(ExecutionError::DeferredCallsError(
                "Call ID does not exist.".into(),
            ));
        };

        // check if the call is already cancelled
        if call.cancelled {
            return Err(ExecutionError::DeferredCallsError(
                "Call ID is already cancelled.".into(),
            ));
        }

        // set call as cancelled
        call.cancelled = true;

        // we need to reimburse coins to the sender
        let res: (Address, Amount) = (call.sender_address, call.coins);

        // Add a cancellation to the current changes
        self.deferred_calls_changes
            .set_call(id.clone(), call.clone());

        let current_gas = self.get_effective_slot_gas(&call.target_slot);

        // set slot gas
        // slot_gas = current_gas - (call_gas + call_cst_gas_cost (vm allocation cost))
        self.deferred_calls_changes.set_effective_slot_gas(
            call.target_slot,
            current_gas.saturating_sub(call.get_effective_gas(self.config.call_cst_gas_cost)),
        );

        let effective_gas_call = call.get_effective_gas(self.config.call_cst_gas_cost) as u128;
        // set total gas
        self.deferred_calls_changes.set_effective_total_gas(
            self.get_effective_total_gas()
                .saturating_sub(effective_gas_call),
        );

        let new_total_calls_registered = self.get_total_calls_registered().saturating_sub(1);
        self.set_total_calls_registered(new_total_calls_registered);

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
    ) -> Result<Amount, ExecutionError> {
        // linear part of the occupancy before booking the requested amount
        let relu_occupancy_before = current_occupancy.saturating_sub(target_occupancy);

        // linear part of the occupancy after booking the requested amount
        let relu_occupancy_after = std::cmp::max(
            current_occupancy.saturating_add(resource_request),
            target_occupancy,
        )
        .saturating_sub(target_occupancy);

        // denominator for the linear fee
        let denominator = resource_supply.checked_sub(target_occupancy).ok_or(
            ExecutionError::DeferredCallsError("Error with denominator on overbooking fee".into()),
        )?;

        if denominator.eq(&0) {
            // TODO : check if this is correct
            return Err(ExecutionError::DeferredCallsError(
                "Denominator is zero on overbooking fee".into(),
            ));
            // OR
            //   return Ok(Amount::from_raw(0));
        }

        // compute using the raw fee and u128 to avoid u64 overflows
        let raw_max_penalty = max_penalty.to_raw() as u128;

        let raw_fee = (raw_max_penalty * relu_occupancy_after / denominator * relu_occupancy_after
            / denominator)
            .saturating_sub(
                raw_max_penalty * relu_occupancy_before / denominator * relu_occupancy_before
                    / denominator,
            );

        Ok(Amount::from_raw(
            std::cmp::min(raw_fee, u64::MAX as u128) as u64
        ))
    }

    /// Compute call fee
    pub fn compute_call_fee(
        &self,
        target_slot: Slot,
        max_gas_request: u64,
        current_slot: Slot,
    ) -> Result<Amount, ExecutionError> {
        // Check that the slot is not in the past
        if target_slot <= current_slot {
            return Err(ExecutionError::DeferredCallsError(
                "Target slot is in the past.".into(),
            ));
        }

        // Check that the slot is not in the future
        if target_slot
            .slots_since(&current_slot, self.config.thread_count)
            .unwrap_or(u64::MAX)
            > self.config.max_future_slots
        {
            // note: the current slot is not counted
            return Err(ExecutionError::DeferredCallsError(
                "Target slot is too far in the future.".into(),
            ));
        }

        // Check that the gas is not too high for the target slot
        let slot_occupancy = self.get_effective_slot_gas(&target_slot);
        if slot_occupancy.saturating_add(max_gas_request) > self.config.max_gas {
            return Err(ExecutionError::DeferredCallsError(
                "Not enough gas available in the target slot.".into(),
            ));
        }

        // We perform Dynamic Pricing of slot gas booking using a Proportional-Integral controller (https://en.wikipedia.org/wiki/Proportional–integral–derivative_controller).
        // It regulates the average slot async gas usage towards `target_async_gas` by adjusting fees.

        // Constant part of the fee: directly depends on the base async gas cost for the target slot.
        // This is the "Integral" part of the Proportional-Integral controller.
        // When a new slot `S` is made available for booking, the `S.base_async_gas_cost` is increased or decreased compared to `(S-1).base_async_gas_cost` depending on the average gas usage over the `deferred_call_max_future_slots` slots before `S`.

        // Integral fee
        let integral_fee = self
            .get_slot_base_fee(&target_slot)
            .saturating_mul_u64(max_gas_request);

        // The integral fee is not enough to respond to quick demand surges within the long booking period `deferred_call_max_future_slots`. Proportional regulation is also necessary.

        // A fee that linearly depends on the total load over `deferred_call_max_future_slots` slots but only when the average load is above `target_async_gas` to not penalize normal use. Booking all the gas from all slots within the booking period requires using the whole initial coin supply.

        // Global overbooking fee
        // TODO check if this is correct
        let global_occupancy = self
            .deferred_calls_changes
            .get_effective_total_gas()
            .unwrap_or_default();
        let global_overbooking_fee = Self::overbooking_fee(
            (self.config.max_gas as u128).saturating_mul(self.config.max_future_slots as u128),
            (self.config.max_future_slots as u128).saturating_mul(TARGET_BOOKING),
            global_occupancy,
            max_gas_request as u128,
            self.config.global_overbooking_penalty, // total_supply
        )?;

        // Finally, a per-slot proportional fee is also added to prevent attackers from denying significant ranges of consecutive slots within the long booking period.
        // Slot overbooking fee
        let slot_overbooking_fee = Self::overbooking_fee(
            self.config.max_gas as u128,
            TARGET_BOOKING,
            slot_occupancy as u128,
            max_gas_request as u128,
            self.config.slot_overbooking_penalty, //   total_initial_coin_supply/10000
        )?;

        // return the fee
        Ok(integral_fee
            .saturating_add(global_overbooking_fee)
            .saturating_add(slot_overbooking_fee))
    }

    /// Register a new call
    /// Returns the call id
    /// # Arguments
    /// * `call` - The call to register
    /// * `trail_hash` - The hash of the execution trail hash
    pub fn register_call(
        &mut self,
        call: DeferredCall,
        trail_hash: massa_hash::Hash,
    ) -> Result<DeferredCallId, ExecutionError> {
        let mut index = 0;

        if let Some(val) = self
            .deferred_calls_changes
            .slots_change
            .get(&call.target_slot)
        {
            index += val.calls_len();
        }

        {
            // final state
            let slots_call = self
                .final_state
                .read()
                .get_deferred_call_registry()
                .get_slot_calls(call.target_slot);
            index += slots_call.slot_calls.len();
        }

        let id = DeferredCallId::new(0, call.target_slot, index as u64, trail_hash.to_bytes())?;

        self.push_new_call(id.clone(), call.clone());

        let current_gas = self.get_effective_slot_gas(&call.target_slot);

        // set slot gas for the target slot
        // effective_slot_gas = current_gas + (call_gas + call_cst_gas_cost (vm allocation cost))
        self.deferred_calls_changes.set_effective_slot_gas(
            call.target_slot,
            current_gas.saturating_add(call.get_effective_gas(self.config.call_cst_gas_cost)),
        );

        // set total effective gas
        let effective_total_gas = self.get_effective_total_gas();
        let call_effective_gas = call.get_effective_gas(self.config.call_cst_gas_cost) as u128;
        self.deferred_calls_changes
            .set_effective_total_gas(effective_total_gas.saturating_add(call_effective_gas));

        // increment total calls registered
        let new_total_calls_registered = self.get_total_calls_registered().saturating_add(1);
        self.set_total_calls_registered(new_total_calls_registered);

        Ok(id)
    }

    /// Take the deferred registry slot changes
    pub(crate) fn take(&mut self) -> DeferredCallRegistryChanges {
        std::mem::take(&mut self.deferred_calls_changes)
    }

    fn set_total_calls_registered(&mut self, nb_calls: u64) {
        massa_metrics::set_deferred_calls_registered(nb_calls as usize);
        self.deferred_calls_changes
            .set_total_calls_registered(nb_calls);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_compute_call_fee() {}
}