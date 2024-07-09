use call::{DeferredCallDeserializer, DeferredCallSerializer};
use massa_db_exports::ShareableMassaDBController;

/// This module implements a new version of the Autonomous Smart Contracts. (ASC)
/// This new version allow asynchronous calls to be registered for a specific slot and ensure his execution.
mod call;

pub use call::DeferredCall;
use massa_ledger_exports::{
    SetOrDelete, SetOrDeleteDeserializer, SetOrDeleteSerializer, SetOrKeep, SetOrKeepDeserializer,
    SetOrKeepSerializer,
};
use massa_models::{
    amount::{Amount, AmountDeserializer, AmountSerializer},
    deferred_call_id::{DeferredCallId, DeferredCallIdDeserializer, DeferredCallIdSerializer},
    slot::Slot,
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Bound::Included;

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

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct DeferredRegistrySlotChanges {
    calls: BTreeMap<DeferredCallId, DeferredRegistryCallChange>,
    gas: DeferredRegistryGasChange<u64>,
    base_fee: DeferredRegistryBaseFeeChange,
}

impl DeferredRegistrySlotChanges {
    pub fn merge(&mut self, other: DeferredRegistrySlotChanges) {
        unimplemented!("DeferredRegistrySlotChanges::merge")
        // for (id, change) in other.calls {
        //     match self.calls.entry(id) {
        //         std::collections::btree_map::Entry::Occupied(mut entry) => {
        //             entry.get_mut().merge(change);
        //         }
        //         std::collections::btree_map::Entry::Vacant(entry) => {
        //             entry.insert(change);
        //         }
        //     }
        // }
        // match other.gas {
        //     DeferredRegistryGasChange::Set(v) => self.gas = DeferredRegistryGasChange::Set(v),
        //     DeferredRegistryGasChange::Keep => {}
        // }
        // match other.base_fee {
        //     DeferredRegistryGasChange::Set(v) => self.base_fee = DeferredRegistryGasChange::Set(v),
        //     DeferredRegistryGasChange::Keep => {}
        // }
    }

    pub fn delete_call(&mut self, id: &DeferredCallId) {
        unimplemented!("DeferredRegistrySlotChanges::delete_call")
        // match self.calls.entry(id.clone()) {
        //     std::collections::btree_map::Entry::Occupied(mut v) => v.get_mut().delete_call(),
        //     std::collections::btree_map::Entry::Vacant(v) => {
        //         v.insert(DeferredRegistryCallChange::Delete);
        //     }
        // }
    }

    pub fn set_call(&mut self, id: DeferredCallId, call: DeferredCall) {
        self.calls.insert(id, DeferredRegistryCallChange::Set(call));
    }

    pub fn get_call(&self, id: &DeferredCallId) -> Option<&DeferredCall> {
        unimplemented!("DeferredRegistrySlotChanges::get_call")
        // self.calls.get(id).and_then(|change| change.get_call())
    }

    pub fn set_gas(&mut self, gas: u64) {
        self.gas = DeferredRegistryGasChange::Set(gas);
    }

    pub fn get_gas(&self) -> Option<u64> {
        match self.gas {
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

pub struct DeferredRegistrySlotChangesSerializer {
    deferred_registry_slot_changes_length: U64VarIntSerializer,
    call_id_serializer: DeferredCallIdSerializer,
    calls_set_or_delete_serializer: SetOrDeleteSerializer<DeferredCall, DeferredCallSerializer>,
    gas_serializer: SetOrKeepSerializer<u64, U64VarIntSerializer>,
    base_fee_serializer: SetOrKeepSerializer<Amount, AmountSerializer>,
}

impl DeferredRegistrySlotChangesSerializer {
    pub fn new() -> Self {
        Self {
            deferred_registry_slot_changes_length: U64VarIntSerializer::new(),
            call_id_serializer: DeferredCallIdSerializer::new(),
            calls_set_or_delete_serializer: SetOrDeleteSerializer::new(
                DeferredCallSerializer::new(),
            ),
            gas_serializer: SetOrKeepSerializer::new(U64VarIntSerializer::new()),
            base_fee_serializer: SetOrKeepSerializer::new(AmountSerializer::new()),
        }
    }
}

impl Serializer<DeferredRegistrySlotChanges> for DeferredRegistrySlotChangesSerializer {
    fn serialize(
        &self,
        value: &DeferredRegistrySlotChanges,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        self.deferred_registry_slot_changes_length.serialize(
            &(value.calls.len().try_into().map_err(|_| {
                SerializeError::GeneralError("Fail to transform usize to u64".to_string())
            })?),
            buffer,
        )?;
        for (id, call) in &value.calls {
            self.call_id_serializer.serialize(id, buffer)?;
            self.calls_set_or_delete_serializer
                .serialize(call, buffer)?;
        }
        self.gas_serializer.serialize(&value.gas, buffer)?;
        self.base_fee_serializer
            .serialize(&value.base_fee, buffer)?;

        Ok(())
    }
}

pub struct DeferredRegistrySlotChangesDeserializer {
    deferred_registry_slot_changes_length: U64VarIntDeserializer,
    call_id_deserializer: DeferredCallIdDeserializer,
    calls_set_or_delete_deserializer:
        SetOrDeleteDeserializer<DeferredCall, DeferredCallDeserializer>,
    gas_deserializer: SetOrKeepDeserializer<u64, U64VarIntDeserializer>,
    base_fee_deserializer: SetOrKeepDeserializer<Amount, AmountDeserializer>,
}

impl DeferredRegistrySlotChangesDeserializer {
    pub fn new(thread_count: u8, max_gas: u64, max_deferred_calls_pool_changes: u64) -> Self {
        Self {
            deferred_registry_slot_changes_length: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_deferred_calls_pool_changes),
            ),
            call_id_deserializer: DeferredCallIdDeserializer::new(),
            calls_set_or_delete_deserializer: SetOrDeleteDeserializer::new(
                DeferredCallDeserializer::new(thread_count),
            ),
            gas_deserializer: SetOrKeepDeserializer::new(U64VarIntDeserializer::new(
                Included(0),
                Included(max_gas),
            )),
            base_fee_deserializer: SetOrKeepDeserializer::new(AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            )),
        }
    }
}

impl Deserializer<DeferredRegistrySlotChanges> for DeferredRegistrySlotChangesDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], DeferredRegistrySlotChanges, E> {
        unimplemented!("DeferredRegistrySlotChangesDeserializer::deserialize")
        // context(
        //     "Failed DeferredRegistrySlotChanges deserialization",
        //     length_count(
        //         context("Failed length deserialization", |input| {
        //             self.async_pool_changes_length.deserialize(input)
        //         }),
        //         |input: &'a [u8]| {
        //             tuple((
        //                 context("Failed id deserialization", |input| {
        //                     self.call_id_deserializer.deserialize(input)
        //                 }),
        //                 context(
        //                     "Failed set_update_or_delete_message deserialization",
        //                     |input| {
        //                         self.set_update_or_delete_message_deserializer
        //                             .deserialize(input)
        //                     },
        //                 ),
        //             ))(input)
        //         },
        //     ),
        // )
        // .map(|vec| {
        //     DeferredRegistrySlotChanges(vec.into_iter().map(|data| (data.0, data.1)).collect())
        // })
        // .parse(buffer)
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct DeferredRegistryChanges {
    pub slots: BTreeMap<Slot, DeferredRegistrySlotChanges>,
    pub total_gas: DeferredRegistryGasChange<u128>,
}

impl DeferredRegistryChanges {
    pub fn merge(&mut self, other: DeferredRegistryChanges) {
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
            DeferredRegistryGasChange::Set(v) => self.total_gas = DeferredRegistryGasChange::Set(v),
            DeferredRegistryGasChange::Keep => {}
        }
    }

    pub fn delete_call(&mut self, target_slot: Slot, id: &DeferredCallId) {
        self.slots.entry(target_slot).or_default().delete_call(id)
    }

    pub fn set_call(&mut self, id: DeferredCallId, call: DeferredCall) {
        self.slots
            .entry(call.target_slot.clone())
            .or_default()
            .set_call(id, call);
    }

    pub fn get_call(&self, target_slot: &Slot, id: &DeferredCallId) -> Option<&DeferredCall> {
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
        self.total_gas = DeferredRegistryGasChange::Set(gas);
    }

    pub fn get_total_gas(&self) -> Option<u128> {
        match self.total_gas {
            DeferredRegistryGasChange::Set(v) => Some(v),
            DeferredRegistryGasChange::Keep => None,
        }
    }
}

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
        let Some(slot_changes) = changes.slots.get(&self.slot) else {
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
