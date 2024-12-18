use massa_models::{
    amount::Amount,
    deferred_calls::DeferredCallId,
    slot::{Slot, SlotDeserializer, SlotSerializer},
    types::{SetOrKeep, SetOrKeepDeserializer, SetOrKeepSerializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U128VarIntDeserializer, U128VarIntSerializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::collections::btree_map::Entry;
use std::{collections::BTreeMap, ops::Bound};

use crate::{
    config::DeferredCallsConfig,
    slot_changes::{
        DeferredRegistrySlotChanges, DeferredRegistrySlotChangesDeserializer,
        DeferredRegistrySlotChangesSerializer,
    },
    DeferredCall, DeferredRegistryGasChange,
};
use std::ops::Bound::Included;

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeferredCallRegistryChanges {
    #[serde_as(as = "Vec<(_, _)>")]
    pub slots_change: BTreeMap<Slot, DeferredRegistrySlotChanges>,

    pub effective_total_gas: DeferredRegistryGasChange<u128>,
    pub total_calls_registered: SetOrKeep<u64>,
}

impl Default for DeferredCallRegistryChanges {
    fn default() -> Self {
        Self {
            slots_change: Default::default(),
            effective_total_gas: DeferredRegistryGasChange::Keep,
            total_calls_registered: SetOrKeep::Keep,
        }
    }
}

impl DeferredCallRegistryChanges {
    pub fn apply(&mut self, changes: DeferredCallRegistryChanges) {
        for (slot, changes) in changes.slots_change.into_iter() {
            match self.slots_change.entry(slot) {
                Entry::Occupied(mut occ) => {
                    // apply incoming change if a change on this entry already exists
                    *occ.get_mut() = changes;
                }
                Entry::Vacant(vac) => {
                    // otherwise insert the incoming change
                    vac.insert(changes);
                }
            }
        }

        self.effective_total_gas = changes.effective_total_gas;
        self.total_calls_registered = changes.total_calls_registered;
    }

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

    pub fn get_call(&self, target_slot: &Slot, id: &DeferredCallId) -> Option<&DeferredCall> {
        self.slots_change
            .get(target_slot)
            .and_then(|slot_changes| slot_changes.get_call(id))
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

    pub fn set_total_calls_registered(&mut self, total_calls_registered: u64) {
        self.total_calls_registered = SetOrKeep::Set(total_calls_registered);
    }

    pub fn get_total_calls_registered(&self) -> Option<u64> {
        match self.total_calls_registered {
            SetOrKeep::Set(v) => Some(v),
            SetOrKeep::Keep => None,
        }
    }
}

pub struct DeferredRegistryChangesSerializer {
    pub(crate) u64_serializer: U64VarIntSerializer,
    slot_changes_serializer: DeferredRegistrySlotChangesSerializer,
    slot_serializer: SlotSerializer,
    pub(crate) effective_total_gas_serializer: SetOrKeepSerializer<u128, U128VarIntSerializer>,
    pub(crate) total_calls_registered_serializer: SetOrKeepSerializer<u64, U64VarIntSerializer>,
}

impl DeferredRegistryChangesSerializer {
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            slot_changes_serializer: DeferredRegistrySlotChangesSerializer::new(),
            slot_serializer: SlotSerializer::new(),
            effective_total_gas_serializer: SetOrKeepSerializer::new(U128VarIntSerializer::new()),
            total_calls_registered_serializer: SetOrKeepSerializer::new(U64VarIntSerializer::new()),
        }
    }
}

impl Default for DeferredRegistryChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<DeferredCallRegistryChanges> for DeferredRegistryChangesSerializer {
    fn serialize(
        &self,
        value: &DeferredCallRegistryChanges,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.u64_serializer.serialize(
            &(value.slots_change.len().try_into().map_err(|_| {
                SerializeError::GeneralError("Fail to transform usize to u64".to_string())
            })?),
            buffer,
        )?;

        for (slot, changes) in &value.slots_change {
            self.slot_serializer.serialize(slot, buffer)?;
            self.slot_changes_serializer.serialize(changes, buffer)?;
        }

        self.effective_total_gas_serializer
            .serialize(&value.effective_total_gas, buffer)?;

        self.total_calls_registered_serializer
            .serialize(&value.total_calls_registered, buffer)?;

        Ok(())
    }
}

pub struct DeferredRegistryChangesDeserializer {
    pub(crate) u64_deserializer: U64VarIntDeserializer,
    slot_changes_deserializer: DeferredRegistrySlotChangesDeserializer,
    slot_deserializer: SlotDeserializer,
    pub(crate) effective_total_gas_deserializer:
        SetOrKeepDeserializer<u128, U128VarIntDeserializer>,
    pub(crate) total_calls_registered_deserializer:
        SetOrKeepDeserializer<u64, U64VarIntDeserializer>,
}

impl DeferredRegistryChangesDeserializer {
    pub fn new(config: DeferredCallsConfig) -> Self {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            slot_changes_deserializer: DeferredRegistrySlotChangesDeserializer::new(config),
            slot_deserializer: SlotDeserializer::new(
                (Bound::Included(0), Bound::Included(u64::MAX)),
                (Bound::Included(0), Bound::Excluded(config.thread_count)),
            ),
            effective_total_gas_deserializer: SetOrKeepDeserializer::new(
                U128VarIntDeserializer::new(Included(u128::MIN), Included(u128::MAX)),
            ),
            total_calls_registered_deserializer: SetOrKeepDeserializer::new(
                U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            ),
        }
    }
}

impl Deserializer<DeferredCallRegistryChanges> for DeferredRegistryChangesDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], DeferredCallRegistryChanges, E> {
        context(
            "Failed DeferredRegistryChanges deserialization",
            tuple((
                length_count(
                    context("Failed length deserialization", |input| {
                        self.u64_deserializer.deserialize(input)
                    }),
                    |input| {
                        tuple((
                            context("Failed slot deserialization", |input| {
                                self.slot_deserializer.deserialize(input)
                            }),
                            context(
                                "Failed set_update_or_delete_message deserialization",
                                |input| self.slot_changes_deserializer.deserialize(input),
                            ),
                        ))(input)
                    },
                ),
                context("Failed total_gas deserialization", |input| {
                    self.effective_total_gas_deserializer.deserialize(input)
                }),
                context("Failed total call registered deserialization", |input| {
                    self.total_calls_registered_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(changes, total_gas, total_calls_registered)| DeferredCallRegistryChanges {
                slots_change: changes.into_iter().collect::<BTreeMap<_, _>>(),
                effective_total_gas: total_gas,
                total_calls_registered,
            },
        )
        .parse(buffer)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use massa_models::{address::Address, amount::Amount, deferred_calls::DeferredCallId};
    use massa_serialization::DeserializeError;

    use crate::{
        config::DeferredCallsConfig,
        registry_changes::{
            DeferredRegistryChangesDeserializer, DeferredRegistryChangesSerializer,
        },
        slot_changes::DeferredRegistrySlotChanges,
        DeferredCall,
    };

    #[test]
    fn test_deferred_registry_ser_deser() {
        use crate::DeferredCallRegistryChanges;
        use massa_models::slot::Slot;
        use massa_serialization::{Deserializer, Serializer};
        use std::collections::BTreeMap;

        let mut changes = DeferredCallRegistryChanges {
            slots_change: BTreeMap::new(),
            effective_total_gas: Default::default(),
            total_calls_registered: Default::default(),
        };

        let mut registry_slot_changes = DeferredRegistrySlotChanges::default();
        registry_slot_changes.set_base_fee(Amount::from_str("100").unwrap());
        registry_slot_changes.set_effective_slot_gas(100_000);
        let target_slot = Slot {
            thread: 5,
            period: 1,
        };

        let call = DeferredCall::new(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            target_slot,
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

        changes
            .slots_change
            .insert(target_slot, registry_slot_changes);

        changes.set_effective_total_gas(100_000);

        let mut buffer = Vec::new();
        let serializer = DeferredRegistryChangesSerializer::new();
        serializer.serialize(&changes, &mut buffer).unwrap();

        let deserializer = DeferredRegistryChangesDeserializer::new(DeferredCallsConfig::default());
        let (rest, deserialized) = deserializer
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();

        assert_eq!(rest.len(), 0);
        let base = changes.slots_change.get(&target_slot).unwrap();
        let slot_changes_deser = deserialized.slots_change.get(&target_slot).unwrap();
        assert_eq!(base.calls, slot_changes_deser.calls);
        assert_eq!(
            changes.effective_total_gas,
            deserialized.effective_total_gas
        );
    }
}
