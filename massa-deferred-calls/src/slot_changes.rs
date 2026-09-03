use std::collections::BTreeMap;

use crate::{
    call::{DeferredCallDeserializer, DeferredCallSerializer},
    config::DeferredCallsConfig,
    DeferredCall, DeferredRegistryBaseFeeChange, DeferredRegistryCallChange,
    DeferredRegistryGasChange,
};
use massa_models::types::{
    SetOrDeleteDeserializer, SetOrDeleteSerializer, SetOrKeepDeserializer, SetOrKeepSerializer,
};
use massa_models::{
    amount::{Amount, AmountDeserializer, AmountSerializer},
    deferred_calls::{DeferredCallId, DeferredCallIdDeserializer, DeferredCallIdSerializer},
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
    pub effective_slot_gas: DeferredRegistryGasChange<u64>,
    pub base_fee: DeferredRegistryBaseFeeChange,
}

impl DeferredRegistrySlotChanges {
    pub fn calls_len(&self) -> usize {
        self.calls.len()
    }

    /// Validates that this structure stays within the same bounds enforced by
    /// [`DeferredRegistrySlotChangesDeserializer`] under `config`.
    pub fn check_bounds(&self, config: &DeferredCallsConfig) -> Result<(), SerializeError> {
        let calls_len: u64 = self.calls.len().try_into().map_err(|_| {
            SerializeError::GeneralError("Fail to transform usize to u64".to_string())
        })?;
        if calls_len > config.max_pool_changes {
            return Err(SerializeError::NumberTooBig(format!(
                "DeferredRegistrySlotChanges calls length {} exceeds max_pool_changes {}",
                calls_len, config.max_pool_changes
            )));
        }
        if let DeferredRegistryGasChange::Set(gas) = self.effective_slot_gas {
            if gas > config.max_gas {
                return Err(SerializeError::NumberTooBig(format!(
                    "DeferredRegistrySlotChanges effective_slot_gas {} exceeds max_gas {}",
                    gas, config.max_gas
                )));
            }
        }
        Ok(())
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

pub struct DeferredRegistrySlotChangesSerializer {
    deferred_registry_slot_changes_length: U64VarIntSerializer,
    call_id_serializer: DeferredCallIdSerializer,
    calls_set_or_delete_serializer: SetOrDeleteSerializer<DeferredCall, DeferredCallSerializer>,
    gas_serializer: SetOrKeepSerializer<u64, U64VarIntSerializer>,
    base_fee_serializer: SetOrKeepSerializer<Amount, AmountSerializer>,
    /// Same bounds as [`DeferredRegistrySlotChangesDeserializer`].
    config: DeferredCallsConfig,
}

impl DeferredRegistrySlotChangesSerializer {
    pub fn new(config: DeferredCallsConfig) -> Self {
        Self {
            deferred_registry_slot_changes_length: U64VarIntSerializer::new(),
            call_id_serializer: DeferredCallIdSerializer::new(),
            calls_set_or_delete_serializer: SetOrDeleteSerializer::new(
                DeferredCallSerializer::new(),
            ),
            gas_serializer: SetOrKeepSerializer::new(U64VarIntSerializer::new()),
            base_fee_serializer: SetOrKeepSerializer::new(AmountSerializer::new()),
            config,
        }
    }
}

impl Default for DeferredRegistrySlotChangesSerializer {
    fn default() -> Self {
        Self::new(DeferredCallsConfig::default())
    }
}

impl Serializer<DeferredRegistrySlotChanges> for DeferredRegistrySlotChangesSerializer {
    fn serialize(
        &self,
        value: &DeferredRegistrySlotChanges,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        // Enforce the same bounds as DeferredRegistrySlotChangesDeserializer.
        value.check_bounds(&self.config)?;
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
        self.gas_serializer
            .serialize(&value.effective_slot_gas, buffer)?;
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
    pub fn new(config: DeferredCallsConfig) -> Self {
        Self {
            deferred_registry_slot_changes_length: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(config.max_pool_changes),
            ),
            call_id_deserializer: DeferredCallIdDeserializer::new(),
            calls_set_or_delete_deserializer: SetOrDeleteDeserializer::new(
                DeferredCallDeserializer::new(config),
            ),
            gas_deserializer: SetOrKeepDeserializer::new(U64VarIntDeserializer::new(
                Included(0),
                Included(config.max_gas),
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
        // Note: unsorted pairs, and duplicate keys (last occurrence wins), on the wire still deserialize
        // to a normalized BTreeMap. This serializer/deserializer pair is only used in tests: production
        // deferred-call processing reads per-key values from the DB and never parses these map blobs.
        // Massa is malleability-resistant by construction, this is not exploitable.
        .map(|(vec, gas, base_fee)| {
            let calls = vec.into_iter().collect::<BTreeMap<_, _>>();

            DeferredRegistrySlotChanges {
                calls,
                effective_slot_gas: gas,
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
        address::Address, amount::Amount, deferred_calls::DeferredCallId, slot::Slot,
    };
    use massa_serialization::{DeserializeError, Deserializer, Serializer};

    use crate::{config::DeferredCallsConfig, DeferredCall, DeferredRegistryGasChange};

    use super::{
        DeferredRegistrySlotChanges, DeferredRegistrySlotChangesDeserializer,
        DeferredRegistrySlotChangesSerializer,
    };

    fn sample_call(target_slot: Slot) -> DeferredCall {
        DeferredCall::new(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            target_slot,
            Address::from_str("AS127QtY6Hzm6BnJc9wqCBfPNvEH9fKer3LiMNNQmcX3MzLwCL6G6").unwrap(),
            "receive".to_string(),
            vec![42, 42, 42, 42],
            Amount::from_raw(100),
            3000000,
            Amount::from_raw(1),
            false,
        )
    }

    #[test]
    fn test_slot_changes_ser_deser() {
        let config = DeferredCallsConfig::default();
        let mut registry_slot_changes = DeferredRegistrySlotChanges::default();
        registry_slot_changes.set_base_fee(Amount::from_str("100").unwrap());
        registry_slot_changes.set_effective_slot_gas(100_000);
        let target_slot = Slot {
            thread: 5,
            period: 1,
        };

        let call = sample_call(target_slot);
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
        let serializer = DeferredRegistrySlotChangesSerializer::new(config);
        serializer
            .serialize(&registry_slot_changes, &mut buffer)
            .unwrap();

        let deserializer = DeferredRegistrySlotChangesDeserializer::new(config);
        let (rest, changes_deser) = deserializer
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();
        assert!(rest.is_empty());
        assert_eq!(changes_deser.calls, registry_slot_changes.calls);
        assert_eq!(
            changes_deser.effective_slot_gas,
            registry_slot_changes.effective_slot_gas
        );
    }

    #[test]
    fn test_slot_changes_serialize_rejects_gas_above_max() {
        let config = DeferredCallsConfig {
            max_gas: 1_000,
            ..DeferredCallsConfig::default()
        };
        let mut changes = DeferredRegistrySlotChanges::default();
        // Mutator still accepts any u64; serializer must reject above max_gas.
        changes.set_effective_slot_gas(config.max_gas + 1);

        let serializer = DeferredRegistrySlotChangesSerializer::new(config);
        let err = serializer
            .serialize(&changes, &mut Vec::new())
            .expect_err("gas above max_gas must fail serialization");
        assert!(
            matches!(err, massa_serialization::SerializeError::NumberTooBig(_)),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn test_slot_changes_serialize_rejects_too_many_calls() {
        let config = DeferredCallsConfig {
            max_pool_changes: 1,
            ..DeferredCallsConfig::default()
        };
        let target_slot = Slot {
            thread: 0,
            period: 1,
        };
        let mut changes = DeferredRegistrySlotChanges::default();
        changes.set_call(
            DeferredCallId::new(0, target_slot, 0, &[]).unwrap(),
            sample_call(target_slot),
        );
        changes.set_call(
            DeferredCallId::new(0, target_slot, 1, &[]).unwrap(),
            sample_call(target_slot),
        );

        let serializer = DeferredRegistrySlotChangesSerializer::new(config);
        let err = serializer
            .serialize(&changes, &mut Vec::new())
            .expect_err("calls above max_pool_changes must fail serialization");
        assert!(
            matches!(err, massa_serialization::SerializeError::NumberTooBig(_)),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn test_slot_changes_ser_deser_at_max_bounds() {
        let config = DeferredCallsConfig {
            max_gas: 42,
            max_pool_changes: 1,
            ..DeferredCallsConfig::default()
        };
        let target_slot = Slot {
            thread: 0,
            period: 1,
        };
        let mut changes = DeferredRegistrySlotChanges::default();
        changes.set_effective_slot_gas(config.max_gas);
        changes.set_call(
            DeferredCallId::new(0, target_slot, 0, &[]).unwrap(),
            sample_call(target_slot),
        );

        let mut buffer = Vec::new();
        DeferredRegistrySlotChangesSerializer::new(config)
            .serialize(&changes, &mut buffer)
            .expect("values at exact bounds must serialize");

        let (rest, decoded) = DeferredRegistrySlotChangesDeserializer::new(config)
            .deserialize::<DeserializeError>(&buffer)
            .expect("values at exact bounds must deserialize");
        assert!(rest.is_empty());
        assert_eq!(decoded.calls.len(), 1);
        assert_eq!(
            decoded.effective_slot_gas,
            DeferredRegistryGasChange::Set(config.max_gas)
        );
    }
}
