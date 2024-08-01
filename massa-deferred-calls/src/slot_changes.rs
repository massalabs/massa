use std::collections::BTreeMap;

use crate::{
    call::{DeferredCallDeserializer, DeferredCallSerializer},
    DeferredCall, DeferredRegistryBaseFeeChange, DeferredRegistryCallChange,
    DeferredRegistryGasChange,
};
use massa_ledger_exports::{
    SetOrDelete, SetOrDeleteDeserializer, SetOrDeleteSerializer, SetOrKeepDeserializer,
    SetOrKeepSerializer,
};
use massa_models::{
    amount::{Amount, AmountDeserializer, AmountSerializer},
    deferred_call_id::{DeferredCallId, DeferredCallIdDeserializer, DeferredCallIdSerializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use serde::{Deserialize, Serialize};
use std::ops::Bound::Included;

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct DeferredRegistrySlotChanges {
    pub calls: BTreeMap<DeferredCallId, DeferredRegistryCallChange>,
    pub gas: DeferredRegistryGasChange<u64>,
    pub base_fee: DeferredRegistryBaseFeeChange,
}

impl DeferredRegistrySlotChanges {
    pub fn calls_len(&self) -> usize {
        self.calls.len()
    }

    pub fn merge(&mut self, _other: DeferredRegistrySlotChanges) {
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

    pub fn get_call(&self, id: &DeferredCallId) -> Option<&DeferredCall> {
        match self.calls.get(id) {
            Some(SetOrDelete::Set(call)) => Some(call),
            _ => None,
        }
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
        context(
            "Failed DeferredRegistrySlotChanges deserialization",
            tuple((
                length_count(
                    context("Failed length deserialization", |input| {
                        self.deferred_registry_slot_changes_length
                            .deserialize(input)
                    }),
                    |input: &'a [u8]| {
                        tuple((
                            context("Failed id deserialization", |input| {
                                self.call_id_deserializer.deserialize(input)
                            }),
                            context(
                                "Failed set_update_or_delete_message deserialization",
                                |input| self.calls_set_or_delete_deserializer.deserialize(input),
                            ),
                        ))(input)
                    },
                ),
                context("Failed gas deserialization", |input| {
                    self.gas_deserializer.deserialize(input)
                }),
                context("Failed base fee deserialize", |input| {
                    self.base_fee_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(vec, gas, base_fee)| {
            let calls = vec.into_iter().collect::<BTreeMap<_, _>>();

            DeferredRegistrySlotChanges {
                calls,
                gas,
                base_fee,
            }
        })
        .parse(buffer)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use massa_models::{
        address::Address, amount::Amount, deferred_call_id::DeferredCallId, slot::Slot,
    };
    use massa_serialization::{DeserializeError, Deserializer, Serializer};

    use crate::DeferredCall;

    use super::{
        DeferredRegistrySlotChanges, DeferredRegistrySlotChangesDeserializer,
        DeferredRegistrySlotChangesSerializer,
    };

    #[test]
    fn test_slot_changes_ser_deser() {
        let mut registry_slot_changes = DeferredRegistrySlotChanges::default();
        registry_slot_changes.set_base_fee(Amount::from_str("100").unwrap());
        registry_slot_changes.set_gas(100_000);
        let target_slot = Slot {
            thread: 5,
            period: 1,
        };

        let call = DeferredCall::new(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            target_slot.clone(),
            Address::from_str("AS127QtY6Hzm6BnJc9wqCBfPNvEH9fKer3LiMNNQmcX3MzLwCL6G6").unwrap(),
            "receive".to_string(),
            vec![42, 42, 42, 42],
            Amount::from_raw(100),
            3000000,
            Amount::from_raw(1),
            false,
        );
        let id = DeferredCallId::new(
            0,
            Slot {
                thread: 5,
                period: 1,
            },
            1,
            &[],
        )
        .unwrap();

        registry_slot_changes.set_call(id, call);

        let mut buffer = Vec::new();
        let serializer = DeferredRegistrySlotChangesSerializer::new();
        serializer
            .serialize(&registry_slot_changes, &mut buffer)
            .unwrap();

        let deserializer = DeferredRegistrySlotChangesDeserializer::new(32, 3000000, 100_000);
        let (rest, changes_deser) = deserializer
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();
        assert!(rest.is_empty());
        assert_eq!(changes_deser.calls, registry_slot_changes.calls);
    }
}
