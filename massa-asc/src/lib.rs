use massa_db_exports::ShareableMassaDBController;

/// This module implements a new version of the Autonomous Smart Contracts. (ASC)
/// This new version allow asynchronous calls to be registered for a specific slot and ensure his execution.
mod call;

pub use call::AsyncCall;
use massa_models::{amount::Amount, asc_call_id::AsyncCallId, slot::Slot};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct AsyncCallRegistry {
    db: ShareableMassaDBController,
}

impl AsyncCallRegistry {
    /*
     DB layout:
        [ASYNC_CALL_TOTAL_GAS_PREFIX] -> u64 // total currently booked gas
        [ASYNC_CAL_SLOT_PREFIX][slot][TOTAL_GAS_TAG] -> u64 // total gas booked for a slot (optional, default 0, deleted when set to 0)
        [ASYNC_CAL_SLOT_PREFIX][slot][CALLS_TAG][id][ASYNC_CALL_FIELD_X_TAG] -> AsyncCall.x // call data
    */

    pub fn new(db: ShareableMassaDBController) -> Self {
        Self { db }
    }

    pub fn get_slot_calls(slot: Slot) -> AsyncSlotCalls {
        todo!()
    }

    pub fn get_call(id: &AsyncCallId) -> Option<AsyncCall> {
        todo!()
    }

    /// Returns the total amount of gas booked for a slot
    pub fn get_slot_gas(slot: Slot) -> u64 {
        // By default, if it is absent, it is 0
        todo!()
    }

    /// Returns the base fee for a slot
    pub fn get_slot_base_fee(slot: Slot) -> Amount {
        // By default, if it is absent, it is 0
        todo!()
    }

    /// Returns the total amount of gas booked
    pub fn get_total_gas() -> u128 {
        todo!()
    }

    pub fn apply_changes(changes: AsyncRegistryChanges) {
        //Note: if a slot gas is zet to 0, delete the slot gas entry
        // same for base fee
        todo!()
    }
}

#[derive(Debug, Clone)]
pub enum AsyncRegistryCallChange {
    Set(AsyncCall),
    Delete,
}

impl AsyncRegistryCallChange {
    pub fn merge(&mut self, other: AsyncRegistryCallChange) {
        *self = other;
    }

    pub fn delete_call(&mut self) {
        *self = AsyncRegistryCallChange::Delete;
    }

    pub fn set_call(&mut self, call: AsyncCall) {
        *self = AsyncRegistryCallChange::Set(call);
    }

    pub fn get_call(&self) -> Option<&AsyncCall> {
        match self {
            AsyncRegistryCallChange::Set(v) => Some(v),
            AsyncRegistryCallChange::Delete => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum AsyncRegistryGasChange<V> {
    Set(V),
    Keep,
}

impl<V> Default for AsyncRegistryGasChange<V> {
    fn default() -> Self {
        AsyncRegistryGasChange::Keep
    }
}

#[derive(Default, Debug, Clone)]
pub struct AsyncRegistrySlotChanges {
    calls: BTreeMap<AsyncCallId, AsyncRegistryCallChange>,
    gas: AsyncRegistryGasChange<u64>,
    base_fee: AsyncRegistryGasChange<Amount>,
}

impl AsyncRegistrySlotChanges {
    pub fn merge(&mut self, other: AsyncRegistrySlotChanges) {
        for (id, change) in other.calls {
            match self.calls.entry(id) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().merge(change);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(change);
                }
            }
        }
        match other.gas {
            AsyncRegistryGasChange::Set(v) => self.gas = AsyncRegistryGasChange::Set(v),
            AsyncRegistryGasChange::Keep => {}
        }
        match other.base_fee {
            AsyncRegistryGasChange::Set(v) => self.base_fee = AsyncRegistryGasChange::Set(v),
            AsyncRegistryGasChange::Keep => {}
        }
    }

    pub fn delete_call(&mut self, id: &AsyncCallId) {
        match self.calls.entry(id.clone()) {
            std::collections::btree_map::Entry::Occupied(mut v) => v.get_mut().delete_call(),
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(AsyncRegistryCallChange::Delete);
            }
        }
    }

    pub fn set_call(&mut self, id: AsyncCallId, call: AsyncCall) {
        self.calls.insert(id, AsyncRegistryCallChange::Set(call));
    }

    pub fn get_call(&self, id: &AsyncCallId) -> Option<&AsyncCall> {
        self.calls.get(id).and_then(|change| change.get_call())
    }

    pub fn set_gas(&mut self, gas: u64) {
        self.gas = AsyncRegistryGasChange::Set(gas);
    }

    pub fn get_gas(&self) -> Option<u64> {
        match self.gas {
            AsyncRegistryGasChange::Set(v) => Some(v),
            AsyncRegistryGasChange::Keep => None,
        }
    }

    pub fn get_base_fee(&self) -> Option<Amount> {
        match self.base_fee {
            AsyncRegistryGasChange::Set(v) => Some(v),
            AsyncRegistryGasChange::Keep => None,
        }
    }

    pub fn set_base_fee(&mut self, base_fee: Amount) {
        self.base_fee = AsyncRegistryGasChange::Set(base_fee);
    }
}

#[derive(Default, Debug, Clone)]
pub struct AsyncRegistryChanges {
    pub slots: BTreeMap<Slot, AsyncRegistrySlotChanges>,
    pub total_gas: AsyncRegistryGasChange<u128>,
}

impl AsyncRegistryChanges {
    pub fn merge(&mut self, other: AsyncRegistryChanges) {
        for (slot, changes) in other.slots {
            match self.slots.entry(slot) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().merge(changes);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(changes);
                }
            }
        }
        match other.total_gas {
            AsyncRegistryGasChange::Set(v) => self.total_gas = AsyncRegistryGasChange::Set(v),
            AsyncRegistryGasChange::Keep => {}
        }
    }

    pub fn delete_call(&mut self, target_slot: Slot, id: &AsyncCallId) {
        self.slots.entry(target_slot).or_default().delete_call(id)
    }

    pub fn set_call(&mut self, id: AsyncCallId, call: AsyncCall) {
        self.slots
            .entry(call.target_slot.clone())
            .or_default()
            .set_call(id, call);
    }

    pub fn get_call(&self, target_slot: &Slot, id: &AsyncCallId) -> Option<&AsyncCall> {
        self.slots
            .get(target_slot)
            .and_then(|slot_changes| slot_changes.get_call(id))
    }

    pub fn get_slot_gas(&self, target_slot: &Slot) -> Option<u64> {
        self.slots
            .get(target_slot)
            .and_then(|slot_changes| slot_changes.get_gas())
    }

    pub fn set_slot_gas(&mut self, target_slot: Slot, gas: u64) {
        self.slots.entry(target_slot).or_default().set_gas(gas);
    }

    pub fn set_slot_base_fee(&mut self, target_slot: Slot, base_fee: Amount) {
        self.slots
            .entry(target_slot)
            .or_default()
            .set_base_fee(base_fee);
    }

    pub fn get_slot_base_fee(&self, target_slot: &Slot) -> Option<Amount> {
        self.slots
            .get(target_slot)
            .and_then(|slot_changes| slot_changes.get_base_fee())
    }

    pub fn set_total_gas(&mut self, gas: u128) {
        self.total_gas = AsyncRegistryGasChange::Set(gas);
    }

    pub fn get_total_gas(&self) -> Option<u128> {
        match self.total_gas {
            AsyncRegistryGasChange::Set(v) => Some(v),
            AsyncRegistryGasChange::Keep => None,
        }
    }
}

/// A structure that lists slot calls for a given slot,
/// as well as global gas usage statistics.
#[derive(Debug, Clone)]
pub struct AsyncSlotCalls {
    pub slot: Slot,
    pub slot_calls: BTreeMap<AsyncCallId, AsyncCall>,
    pub slot_gas: u64,
    pub slot_base_fee: Amount,
    pub total_gas: u128,
}

impl AsyncSlotCalls {
    pub fn new(slot: Slot) -> Self {
        Self {
            slot,
            slot_calls: BTreeMap::new(),
            slot_gas: 0,
            slot_base_fee: Amount::zero(),
            total_gas: 0,
        }
    }

    pub fn apply_changes(&mut self, changes: &AsyncRegistryChanges) {
        let Some(slot_changes) = changes.slots.get(&self.slot) else {
            return;
        };
        for (id, change) in &slot_changes.calls {
            match change {
                AsyncRegistryCallChange::Set(call) => {
                    self.slot_calls.insert(id.clone(), call.clone());
                }
                AsyncRegistryCallChange::Delete => {
                    self.slot_calls.remove(id);
                }
            }
        }
        match slot_changes.gas {
            AsyncRegistryGasChange::Set(v) => self.slot_gas = v,
            AsyncRegistryGasChange::Keep => {}
        }
        match slot_changes.base_fee {
            AsyncRegistryGasChange::Set(v) => self.slot_base_fee = v,
            AsyncRegistryGasChange::Keep => {}
        }
        match changes.total_gas {
            AsyncRegistryGasChange::Set(v) => self.total_gas = v,
            AsyncRegistryGasChange::Keep => {}
        }
    }
}
