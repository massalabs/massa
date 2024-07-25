use call::{DeferredCallDeserializer, DeferredCallSerializer};
use macros::DEFERRED_CALL_TOTAL_GAS;
use massa_db_exports::{
    DBBatch, ShareableMassaDBController, CRUD_ERROR, DEFERRED_CALLS_SLOT_PREFIX, KEY_DESER_ERROR,
    MESSAGE_DESER_ERROR, MESSAGE_SER_ERROR, STATE_CF,
};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use registry_changes::{
    DeferredRegistryChanges, DeferredRegistryChangesDeserializer, DeferredRegistryChangesSerializer,
};

/// This module implements a new version of the Autonomous Smart Contracts. (ASC)
/// This new version allow asynchronous calls to be registered for a specific slot and ensure his execution.
mod call;
pub mod registry_changes;
pub mod slot_changes;

#[cfg(test)]
mod tests;

#[macro_use]
mod macros;

pub use call::DeferredCall;
use massa_ledger_exports::{SetOrDelete, SetOrKeep};
use massa_models::{
    amount::Amount,
    config::{DEFERRED_CALL_MAX_POOL_CHANGES, MAX_ASYNC_GAS, THREAD_COUNT},
    deferred_call_id::{DeferredCallId, DeferredCallIdDeserializer, DeferredCallIdSerializer},
    slot::Slot,
};
use std::collections::BTreeMap;

// #[derive(Debug)]
pub struct DeferredCallRegistry {
    db: ShareableMassaDBController,
    call_serializer: DeferredCallSerializer,
    call_id_serializer: DeferredCallIdSerializer,
    call_deserializer: DeferredCallDeserializer,
    call_id_deserializer: DeferredCallIdDeserializer,
    registry_changes_deserializer: DeferredRegistryChangesDeserializer,
    registry_changes_serializer: DeferredRegistryChangesSerializer,
}

impl DeferredCallRegistry {
    /*
     DB layout:
        [DEFERRED_CALL_TOTAL_GAS] -> u64 // total currently booked gas
        [DEFERRED_CALLS_SLOT_PREFIX][slot][SLOT_TOTAL_GAS] -> u64 // total gas booked for a slot (optional, default 0, deleted when set to 0)
        [DEFERRED_CALLS_SLOT_PREFIX][slot][SLOT_BASE_FEE] -> u64 // deleted when set to 0
        [DEFERRED_CALLS_SLOT_PREFIX][slot][CALLS_TAG][id][CALL_FIELD_X_TAG] -> AsyncCall.x // call data
    */

    // TODO pass args
    pub fn new(db: ShareableMassaDBController) -> Self {
        Self {
            db,
            call_serializer: DeferredCallSerializer::new(),
            call_id_serializer: DeferredCallIdSerializer::new(),
            call_deserializer: DeferredCallDeserializer::new(THREAD_COUNT),
            call_id_deserializer: DeferredCallIdDeserializer::new(),
            registry_changes_deserializer: DeferredRegistryChangesDeserializer::new(
                THREAD_COUNT,
                MAX_ASYNC_GAS,
                DEFERRED_CALL_MAX_POOL_CHANGES,
            ),
            registry_changes_serializer: DeferredRegistryChangesSerializer::new(),
        }
    }

    pub fn get_slot_calls(&self, slot: Slot) -> DeferredSlotCalls {
        let mut to_return = DeferredSlotCalls::new(slot);
        let key = deferred_slot_call_prefix_key!(slot.to_bytes_key());

        // cache the call ids to avoid duplicates iteration
        let mut temp = Vec::new();

        for (serialized_key, _serialized_value) in self.db.read().prefix_iterator_cf(STATE_CF, &key)
        {
            if !serialized_key.starts_with(&key) {
                break;
            }

            let rest_key = &serialized_key[key.len()..];

            let (_rest, call_id) = self
                .call_id_deserializer
                .deserialize::<DeserializeError>(&rest_key)
                .expect(KEY_DESER_ERROR);

            if temp.contains(&call_id) {
                continue;
            }

            temp.push(call_id.clone());

            if let Some(call) = self.get_call(&slot, &call_id) {
                to_return.slot_calls.insert(call_id, call);
            }
        }

        to_return.slot_base_fee = self.get_slot_base_fee(&slot);
        to_return.slot_gas = self.get_slot_gas(&slot);
        to_return.total_gas = self.get_total_gas();

        to_return
    }

    /// Returns the DeferredCall for a given slot and id
    pub fn get_call(&self, slot: &Slot, id: &DeferredCallId) -> Option<DeferredCall> {
        let mut buf_id = Vec::new();
        self.call_id_serializer
            .serialize(id, &mut buf_id)
            .expect(MESSAGE_SER_ERROR);
        let key = deferred_call_prefix_key!(buf_id, slot.to_bytes_key());

        let mut serialized_call: Vec<u8> = Vec::new();
        for (serialized_key, serialized_value) in self.db.read().prefix_iterator_cf(STATE_CF, &key)
        {
            if !serialized_key.starts_with(&key) {
                break;
            }

            serialized_call.extend(serialized_value.iter());
        }

        match self
            .call_deserializer
            .deserialize::<DeserializeError>(&serialized_call)
        {
            Ok((_rest, call)) => Some(call),
            Err(_) => None,
        }
    }

    /// Returns the total amount of gas booked for a slot
    pub fn get_slot_gas(&self, slot: &Slot) -> u64 {
        // By default, if it is absent, it is 0
        let key = deferred_call_slot_total_gas_key!(slot.to_bytes_key());
        match self.db.read().get_cf(STATE_CF, key) {
            Ok(Some(v)) => {
                let result = self
                    .call_deserializer
                    .u64_var_int_deserializer
                    .deserialize::<DeserializeError>(&v)
                    .expect(MESSAGE_DESER_ERROR)
                    .1;
                result
            }
            _ => 0,
        }
    }

    /// Returns the base fee for a slot
    pub fn get_slot_base_fee(&self, slot: &Slot) -> Amount {
        let key = deferred_call_slot_base_fee_key!(slot.to_bytes_key());
        match self.db.read().get_cf(STATE_CF, key) {
            Ok(Some(v)) => {
                self.call_deserializer
                    .amount_deserializer
                    .deserialize::<DeserializeError>(&v)
                    .expect(MESSAGE_DESER_ERROR)
                    .1
            }
            _ => Amount::zero(),
        }
    }

    /// Returns the total amount of gas booked
    pub fn get_total_gas(&self) -> u128 {
        match self
            .db
            .read()
            .get_cf(STATE_CF, DEFERRED_CALL_TOTAL_GAS.as_bytes().to_vec())
            .expect(CRUD_ERROR)
        {
            Some(v) => {
                let result = self
                    .registry_changes_deserializer
                    .total_gas_deserializer
                    .deserialize::<DeserializeError>(&v)
                    .expect(MESSAGE_DESER_ERROR)
                    .1;
                match result {
                    SetOrKeep::Set(v) => v,
                    SetOrKeep::Keep => 0,
                }
            }
            None => 0,
        }
    }

    pub fn put_entry(
        &self,
        slot: &Slot,
        call_id: &DeferredCallId,
        call: &DeferredCall,
        batch: &mut DBBatch,
    ) {
        let mut buffer_id = Vec::new();
        self.call_id_serializer
            .serialize(call_id, &mut buffer_id)
            .expect(MESSAGE_SER_ERROR);

        let slot_bytes = slot.to_bytes_key();

        let db = self.db.read();

        // sender address
        let mut temp_buffer = Vec::new();
        self.call_serializer
            .address_serializer
            .serialize(&call.sender_address, &mut temp_buffer)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(
            batch,
            sender_address_key!(buffer_id, slot_bytes),
            &temp_buffer,
        );
        temp_buffer.clear();

        // target slot
        self.call_serializer
            .slot_serializer
            .serialize(&call.target_slot, &mut temp_buffer)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(batch, target_slot_key!(buffer_id, slot_bytes), &temp_buffer);
        temp_buffer.clear();

        // target address
        self.call_serializer
            .address_serializer
            .serialize(&call.target_address, &mut temp_buffer)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(
            batch,
            target_address_key!(buffer_id, slot_bytes),
            &temp_buffer,
        );
        temp_buffer.clear();

        // target function
        self.call_serializer
            .string_serializer
            .serialize(&call.target_function, &mut temp_buffer)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(
            batch,
            target_function_key!(buffer_id, slot_bytes),
            &temp_buffer,
        );
        temp_buffer.clear();

        // parameters
        self.call_serializer
            .vec_u8_serializer
            .serialize(&call.parameters, &mut temp_buffer)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(batch, parameters_key!(buffer_id, slot_bytes), &temp_buffer);
        temp_buffer.clear();

        // coins
        self.call_serializer
            .amount_serializer
            .serialize(&call.coins, &mut temp_buffer)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(batch, coins_key!(buffer_id, slot_bytes), &temp_buffer);
        temp_buffer.clear();

        // max gas
        self.call_serializer
            .u64_var_int_serializer
            .serialize(&call.max_gas, &mut temp_buffer)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(batch, max_gas_key!(buffer_id, slot_bytes), &temp_buffer);
        temp_buffer.clear();

        // fee
        self.call_serializer
            .amount_serializer
            .serialize(&call.fee, &mut temp_buffer)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(batch, fee_key!(buffer_id, slot_bytes), &temp_buffer);
        temp_buffer.clear();

        // cancelled
        self.call_serializer
            .bool_serializer
            .serialize(&call.cancelled, &mut temp_buffer)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(batch, cancelled_key!(buffer_id, slot_bytes), &temp_buffer);
    }

    fn delete_entry(&self, id: &DeferredCallId, slot: &Slot, batch: &mut DBBatch) {
        let mut buffer_id = Vec::new();
        self.call_id_serializer
            .serialize(id, &mut buffer_id)
            .expect(MESSAGE_SER_ERROR);

        let slot_bytes = slot.to_bytes_key();

        let db = self.db.read();

        db.delete_key(batch, sender_address_key!(buffer_id, slot_bytes));
        db.delete_key(batch, target_slot_key!(buffer_id, slot_bytes));
        db.delete_key(batch, target_address_key!(buffer_id, slot_bytes));
        db.delete_key(batch, target_function_key!(buffer_id, slot_bytes));
        db.delete_key(batch, parameters_key!(buffer_id, slot_bytes));
        db.delete_key(batch, coins_key!(buffer_id, slot_bytes));
        db.delete_key(batch, max_gas_key!(buffer_id, slot_bytes));
        db.delete_key(batch, fee_key!(buffer_id, slot_bytes));
        db.delete_key(batch, cancelled_key!(buffer_id, slot_bytes));
    }

    pub fn apply_changes_to_batch(&self, changes: DeferredRegistryChanges, batch: &mut DBBatch) {
        //Note: if a slot gas is zet to 0, delete the slot gas entry
        // same for base fee

        for change in changes.slots_change.iter() {
            let slot = change.0;
            let slot_changes = change.1;
            for (id, call_change) in slot_changes.calls.iter() {
                match call_change {
                    DeferredRegistryCallChange::Set(call) => {
                        self.put_entry(slot, id, call, batch);
                    }
                    DeferredRegistryCallChange::Delete => {
                        self.delete_entry(id, slot, batch);
                    }
                }
            }
            match slot_changes.gas {
                DeferredRegistryGasChange::Set(v) => {
                    let key = deferred_call_slot_total_gas_key!(slot.to_bytes_key());
                    //Note: if a slot gas is zet to 0, delete the slot gas entry
                    if v.eq(&0) {
                        self.db.read().delete_key(batch, key);
                    } else {
                        let mut value_ser = Vec::new();
                        self.call_serializer
                            .u64_var_int_serializer
                            .serialize(&v, &mut value_ser)
                            .expect(MESSAGE_SER_ERROR);
                        self.db
                            .read()
                            .put_or_update_entry_value(batch, key, &value_ser);
                    }
                }
                DeferredRegistryGasChange::Keep => {}
            }
            match slot_changes.base_fee {
                DeferredRegistryBaseFeeChange::Set(v) => {
                    let key = deferred_call_slot_base_fee_key!(slot.to_bytes_key());
                    //Note: if a base fee is zet to 0, delete the base fee entry
                    if v.eq(&Amount::zero()) {
                        self.db.read().delete_key(batch, key);
                    } else {
                        let mut value_ser = Vec::new();
                        self.call_serializer
                            .amount_serializer
                            .serialize(&v, &mut value_ser)
                            .expect(MESSAGE_SER_ERROR);
                        self.db
                            .read()
                            .put_or_update_entry_value(batch, key, &value_ser);
                    }
                }
                DeferredRegistryBaseFeeChange::Keep => {}
            }
        }

        match changes.total_gas {
            DeferredRegistryGasChange::Set(v) => {
                let key = DEFERRED_CALL_TOTAL_GAS.as_bytes().to_vec();
                let mut value_ser = Vec::new();
                self.registry_changes_serializer
                    .total_gas_serializer
                    .serialize(&DeferredRegistryGasChange::Set(v), &mut value_ser)
                    .expect(MESSAGE_SER_ERROR);
                self.db
                    .read()
                    .put_or_update_entry_value(batch, key, &value_ser);
            }
            DeferredRegistryGasChange::Keep => {}
        }
    }
}

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub enum DeferredRegistryCallChange {
//     Set(DeferredCall),
//     Delete,
// }

// TODO put SetOrDelete dans models
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
