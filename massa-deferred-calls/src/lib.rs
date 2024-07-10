use massa_db_exports::ShareableMassaDBController;
use registry_changes::DeferredRegistryChanges;

/// This module implements a new version of the Autonomous Smart Contracts. (ASC)
/// This new version allow asynchronous calls to be registered for a specific slot and ensure his execution.
mod call;
pub mod registry_changes;
pub mod slot_changes;

pub use call::DeferredCall;
use massa_ledger_exports::{SetOrDelete, SetOrKeep};
use massa_models::{
    amount::Amount,
    deferred_call_id::{DeferredCallId, DeferredCallIdDeserializer, DeferredCallIdSerializer},
    slot::{Slot, SlotSerializer},
};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct DeferredCallRegistry {
    db: ShareableMassaDBController,
}

impl DeferredCallRegistry {
    /*
     DB layout:
        [ASYNC_CALL_TOTAL_GAS_PREFIX] -> u64 // total currently booked gas
        [ASYNC_CAL_SLOT_PREFIX][slot][TOTAL_GAS_TAG] -> u64 // total gas booked for a slot (optional, default 0, deleted when set to 0)
        [ASYNC_CAL_SLOT_PREFIX][slot][CALLS_TAG][id][ASYNC_CALL_FIELD_X_TAG] -> AsyncCall.x // call data
    */

    pub fn new(db: ShareableMassaDBController) -> Self {
        Self { db }
    }

    pub fn get_slot_calls(&self, slot: Slot) -> DeferredSlotCalls {
        todo!()
    }

    pub fn get_call(&self, slot: &Slot, id: &DeferredCallId) -> Option<DeferredCall> {
        todo!()
    }

    /// Returns the total amount of gas booked for a slot
    pub fn get_slot_gas(slot: Slot) -> u64 {
        // By default, if it is absent, it is 0
        todo!()
    }

    /// Returns the base fee for a slot
    pub fn get_slot_base_fee(&self, slot: &Slot) -> Amount {
        // By default, if it is absent, it is 0
        todo!()
    }

    /// Returns the total amount of gas booked
    pub fn get_total_gas() -> u128 {
        todo!()
    }

    pub fn apply_changes(changes: DeferredRegistryChanges) {
        //Note: if a slot gas is zet to 0, delete the slot gas entry
        // same for base fee
        todo!()
    }
}

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub enum DeferredRegistryCallChange {
//     Set(DeferredCall),
//     Delete,
// }

// todo put SetOrDelete dans models
pub type DeferredRegistryCallChange = SetOrDelete<DeferredCall>;
pub type DeferredRegistryGasChange<V> = SetOrKeep<V>;
pub type DeferredRegistryBaseFeeChange = SetOrKeep<Amount>;

// impl DeferredRegistryCallChange {
//     pub fn merge(&mut self, other: DeferredRegistryCallChange) {
//         *self = other;
//     }

//     pub fn delete_call(&mut self) {
//         *self = DeferredRegistryCallChange::Delete;
//     }

//     pub fn set_call(&mut self, call: DeferredCall) {
//         *self = DeferredRegistryCallChange::Set(call);
//     }

//     pub fn get_call(&self) -> Option<&DeferredCall> {
//         match self {
//             DeferredRegistryCallChange::Set(v) => Some(v),
//             DeferredRegistryCallChange::Delete => None,
//         }
//     }
// }

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub enum DeferredRegistryGasChange<V> {
//     Set(V),
//     Keep,
// }

// impl<V> Default for DeferredRegistryGasChange<V> {
//     fn default() -> Self {
//         DeferredRegistryGasChange::Keep
//     }
// }

/// A structure that lists slot calls for a given slot,
/// as well as global gas usage statistics.
#[derive(Debug, Clone)]
pub struct DeferredSlotCalls {
    pub slot: Slot,
    pub slot_calls: BTreeMap<DeferredCallId, DeferredCall>,
    pub slot_gas: u64,
    pub slot_base_fee: Amount,
    pub total_gas: u128,
}

impl DeferredSlotCalls {
    pub fn new(slot: Slot) -> Self {
        Self {
            slot,
            slot_calls: BTreeMap::new(),
            slot_gas: 0,
            slot_base_fee: Amount::zero(),
            total_gas: 0,
        }
    }

    pub fn apply_changes(&mut self, changes: &DeferredRegistryChanges) {
        let Some(slot_changes) = changes.slots_change.get(&self.slot) else {
            return;
        };
        for (id, change) in &slot_changes.calls {
            match change {
                DeferredRegistryCallChange::Set(call) => {
                    self.slot_calls.insert(id.clone(), call.clone());
                }
                DeferredRegistryCallChange::Delete => {
                    self.slot_calls.remove(id);
                }
            }
        }
        match slot_changes.gas {
            DeferredRegistryGasChange::Set(v) => self.slot_gas = v,
            DeferredRegistryGasChange::Keep => {}
        }
        match slot_changes.base_fee {
            DeferredRegistryGasChange::Set(v) => self.slot_base_fee = v,
            DeferredRegistryGasChange::Keep => {}
        }
        match changes.total_gas {
            DeferredRegistryGasChange::Set(v) => self.total_gas = v,
            DeferredRegistryGasChange::Keep => {}
        }
    }
}
