use std::collections::BTreeMap;

use crate::{
    DeferredCall, DeferredRegistryBaseFeeChange, DeferredRegistryCallChange,
    DeferredRegistryGasChange,
};
use massa_models::{amount::Amount, deferred_calls::DeferredCallId};
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct DeferredRegistrySlotChanges {
    pub calls: BTreeMap<DeferredCallId, DeferredRegistryCallChange>,
    pub effective_slot_gas: DeferredRegistryGasChange<u64>,
    pub base_fee: DeferredRegistryBaseFeeChange,
}

impl DeferredRegistrySlotChanges {
    pub fn calls_len(&self) -> usize {
        self.calls.len()
    }

    /// add Delete changes will delete the call from the db registry when the slot is finalized
    pub fn delete_call(&mut self, id: &DeferredCallId) {
        match self.calls.entry(id.clone()) {
            std::collections::btree_map::Entry::Occupied(mut v) => {
                *v.get_mut() = DeferredRegistryCallChange::Delete;
            }
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(DeferredRegistryCallChange::Delete);
            }
        }
    }

    pub fn set_call(&mut self, id: DeferredCallId, call: DeferredCall) {
        self.calls.insert(id, DeferredRegistryCallChange::Set(call));
    }

    /// Returns the raw change entry for `id` so that callers can distinguish
    /// `Set` (present), `Delete` (tombstoned), and `None` (no change recorded
    /// in this layer). Required to stop speculative lookup cascades when a
    /// deferred call has been deleted in a newer layer.
    pub fn get_call_change(&self, id: &DeferredCallId) -> Option<&DeferredRegistryCallChange> {
        self.calls.get(id)
    }

    pub fn set_effective_slot_gas(&mut self, gas: u64) {
        self.effective_slot_gas = DeferredRegistryGasChange::Set(gas);
    }

    pub fn get_effective_slot_gas(&self) -> Option<u64> {
        match self.effective_slot_gas {
            DeferredRegistryGasChange::Set(v) => Some(v),
            DeferredRegistryGasChange::Keep => None,
        }
    }

    pub fn get_base_fee(&self) -> Option<Amount> {
        match self.base_fee {
            DeferredRegistryGasChange::Set(v) => Some(v),
            DeferredRegistryGasChange::Keep => None,
        }
    }

    pub fn set_base_fee(&mut self, base_fee: Amount) {
        self.base_fee = DeferredRegistryGasChange::Set(base_fee);
    }
}
