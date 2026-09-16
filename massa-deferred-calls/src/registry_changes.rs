use massa_models::{amount::Amount, deferred_calls::DeferredCallId, slot::Slot};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::collections::BTreeMap;

use crate::{
    slot_changes::DeferredRegistrySlotChanges, DeferredCall, DeferredRegistryCallChange,
    DeferredRegistryGasChange,
};

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeferredCallRegistryChanges {
    #[serde_as(as = "Vec<(_, _)>")]
    pub slots_change: BTreeMap<Slot, DeferredRegistrySlotChanges>,

    pub effective_total_gas: DeferredRegistryGasChange<u128>,
    // stats : (success, failed, cancel)
    pub exec_stats: (u64, u64, u64),
}

impl Default for DeferredCallRegistryChanges {
    fn default() -> Self {
        Self {
            slots_change: Default::default(),
            effective_total_gas: DeferredRegistryGasChange::Keep,
            exec_stats: (0, 0, 0),
        }
    }
}

impl DeferredCallRegistryChanges {
    pub fn delete_call(&mut self, target_slot: Slot, id: &DeferredCallId) {
        self.slots_change
            .entry(target_slot)
            .or_default()
            .delete_call(id)
    }

    pub fn set_call(&mut self, id: DeferredCallId, call: DeferredCall) {
        self.slots_change
            .entry(call.target_slot)
            .or_default()
            .set_call(id, call);
    }

    /// Returns the raw change entry for `(target_slot, id)` so that callers
    /// can distinguish `Set` (present), `Delete` (tombstoned) and `None`
    pub fn get_call_change(
        &self,
        target_slot: &Slot,
        id: &DeferredCallId,
    ) -> Option<&DeferredRegistryCallChange> {
        self.slots_change
            .get(target_slot)
            .and_then(|slot_changes| slot_changes.get_call_change(id))
    }

    pub fn get_effective_slot_gas(&self, target_slot: &Slot) -> Option<u64> {
        self.slots_change
            .get(target_slot)
            .and_then(|slot_changes| slot_changes.get_effective_slot_gas())
    }

    pub fn set_effective_slot_gas(&mut self, target_slot: Slot, gas: u64) {
        self.slots_change
            .entry(target_slot)
            .or_default()
            .set_effective_slot_gas(gas);
    }

    pub fn set_slot_base_fee(&mut self, target_slot: Slot, base_fee: Amount) {
        self.slots_change
            .entry(target_slot)
            .or_default()
            .set_base_fee(base_fee);
    }

    pub fn get_slot_base_fee(&self, target_slot: &Slot) -> Option<Amount> {
        self.slots_change
            .get(target_slot)
            .and_then(|slot_changes| slot_changes.get_base_fee())
    }

    pub fn set_effective_total_gas(&mut self, gas: u128) {
        self.effective_total_gas = DeferredRegistryGasChange::Set(gas);
    }

    pub fn get_effective_total_gas(&self) -> Option<u128> {
        match self.effective_total_gas {
            DeferredRegistryGasChange::Set(v) => Some(v),
            DeferredRegistryGasChange::Keep => None,
        }
    }
}
