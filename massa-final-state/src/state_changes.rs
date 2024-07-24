//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file provides structures representing changes to the final state

use massa_async_pool::{
    AsyncPoolChanges, AsyncPoolChangesDeserializer, AsyncPoolChangesSerializer,
};
use massa_deferred_calls::registry_changes::{
    DeferredRegistryChanges, DeferredRegistryChangesDeserializer, DeferredRegistryChangesSerializer,
};
use massa_executed_ops::{
    ExecutedDenunciationsChanges, ExecutedDenunciationsChangesDeserializer,
    ExecutedDenunciationsChangesSerializer, ExecutedOpsChanges, ExecutedOpsChangesDeserializer,
    ExecutedOpsChangesSerializer,
};
use massa_hash::{HashDeserializer, HashSerializer};
use massa_ledger_exports::{
    LedgerChanges, LedgerChangesDeserializer, LedgerChangesSerializer, SetOrKeep,
    SetOrKeepDeserializer, SetOrKeepSerializer,
};
use massa_pos_exports::{PoSChanges, PoSChangesDeserializer, PoSChangesSerializer};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};
use serde::{Deserialize, Serialize};

/// represents changes that can be applied to the execution state
#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct StateChanges {
    /// ledger changes
    pub ledger_changes: LedgerChanges,
    /// asynchronous pool changes
    pub async_pool_changes: AsyncPoolChanges,
    /// deferred call changes
    pub deferred_call_changes: DeferredRegistryChanges,
    /// roll state changes
    pub pos_changes: PoSChanges,
    /// executed operations changes
    pub executed_ops_changes: ExecutedOpsChanges,
    /// executed denunciations changes
    pub executed_denunciations_changes: ExecutedDenunciationsChanges,
    /// execution trail hash change
    pub execution_trail_hash_change: SetOrKeep<massa_hash::Hash>,
}

/// Basic `StateChanges` serializer.
pub struct StateChangesSerializer {
    ledger_changes_serializer: LedgerChangesSerializer,
    async_pool_changes_serializer: AsyncPoolChangesSerializer,
    deferred_call_changes_serializer: DeferredRegistryChangesSerializer,
    pos_changes_serializer: PoSChangesSerializer,
    ops_changes_serializer: ExecutedOpsChangesSerializer,
    de_changes_serializer: ExecutedDenunciationsChangesSerializer,
    execution_trail_hash_change_serializer: SetOrKeepSerializer<massa_hash::Hash, HashSerializer>,
}

impl Default for StateChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl StateChangesSerializer {
    /// Creates a `StateChangesSerializer`
    pub fn new() -> Self {
        Self {
            ledger_changes_serializer: LedgerChangesSerializer::new(),
            async_pool_changes_serializer: AsyncPoolChangesSerializer::new(),
            deferred_call_changes_serializer: DeferredRegistryChangesSerializer::new(),
            pos_changes_serializer: PoSChangesSerializer::new(),
            ops_changes_serializer: ExecutedOpsChangesSerializer::new(),
            de_changes_serializer: ExecutedDenunciationsChangesSerializer::new(),
            execution_trail_hash_change_serializer: SetOrKeepSerializer::new(HashSerializer::new()),
        }
    }
}

impl Serializer<StateChanges> for StateChangesSerializer {
    fn serialize(&self, value: &StateChanges, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.ledger_changes_serializer
            .serialize(&value.ledger_changes, buffer)?;
        self.async_pool_changes_serializer
            .serialize(&value.async_pool_changes, buffer)?;
        self.deferred_call_changes_serializer
            .serialize(&value.deferred_call_changes, buffer)?;
        self.pos_changes_serializer
            .serialize(&value.pos_changes, buffer)?;
        self.ops_changes_serializer
            .serialize(&value.executed_ops_changes, buffer)?;
        self.de_changes_serializer
            .serialize(&value.executed_denunciations_changes, buffer)?;
        self.execution_trail_hash_change_serializer
            .serialize(&value.execution_trail_hash_change, buffer)?;
        Ok(())
    }
}

/// Basic `StateChanges` deserializer
pub struct StateChangesDeserializer {
    ledger_changes_deserializer: LedgerChangesDeserializer,
    async_pool_changes_deserializer: AsyncPoolChangesDeserializer,
    deferred_call_changes_deserializer: DeferredRegistryChangesDeserializer,
    pos_changes_deserializer: PoSChangesDeserializer,
    ops_changes_deserializer: ExecutedOpsChangesDeserializer,
    de_changes_deserializer: ExecutedDenunciationsChangesDeserializer,
    execution_trail_hash_change_deserializer:
        SetOrKeepDeserializer<massa_hash::Hash, HashDeserializer>,
}

impl StateChangesDeserializer {
    /// Creates a `StateChangesDeserializer`
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        thread_count: u8,
        max_async_pool_changes: u64,
        max_function_length: u16,
        max_function_params_length: u64,
        max_ledger_changes_count: u64,
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
        max_datastore_entry_count: u64,
        max_rolls_length: u64,
        max_production_stats_length: u64,
        max_credits_length: u64,
        max_ops_changes_length: u64,
        endorsement_count: u32,
        max_de_changes_length: u64,
        max_deferred_call_pool_changes: u64,
    ) -> Self {
        Self {
            ledger_changes_deserializer: LedgerChangesDeserializer::new(
                max_ledger_changes_count,
                max_datastore_key_length,
                max_datastore_value_length,
                max_datastore_entry_count,
            ),
            async_pool_changes_deserializer: AsyncPoolChangesDeserializer::new(
                thread_count,
                max_async_pool_changes,
                max_function_length,
                max_function_params_length,
                max_datastore_key_length as u32,
            ),
            // todo max gas
            deferred_call_changes_deserializer: DeferredRegistryChangesDeserializer::new(
                thread_count,
                u64::MAX,
                max_deferred_call_pool_changes,
            ),
            pos_changes_deserializer: PoSChangesDeserializer::new(
                thread_count,
                max_rolls_length,
                max_production_stats_length,
                max_credits_length,
            ),
            ops_changes_deserializer: ExecutedOpsChangesDeserializer::new(
                thread_count,
                max_ops_changes_length,
            ),
            de_changes_deserializer: ExecutedDenunciationsChangesDeserializer::new(
                thread_count,
                endorsement_count,
                max_de_changes_length,
            ),
            execution_trail_hash_change_deserializer: SetOrKeepDeserializer::new(
                HashDeserializer::new(),
            ),
        }
    }
}

impl Deserializer<StateChanges> for StateChangesDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], StateChanges, E> {
        context(
            "Failed StateChanges deserialization",
            tuple((
                context("Failed ledger_changes deserialization", |input| {
                    self.ledger_changes_deserializer.deserialize(input)
                }),
                context("Failed async_pool_changes deserialization", |input| {
                    self.async_pool_changes_deserializer.deserialize(input)
                }),
                context("Failed deferred_call_changes deserialization", |input| {
                    self.deferred_call_changes_deserializer.deserialize(input)
                }),
                context("Failed roll_state_changes deserialization", |input| {
                    self.pos_changes_deserializer.deserialize(input)
                }),
                context("Failed executed_ops_changes deserialization", |input| {
                    self.ops_changes_deserializer.deserialize(input)
                }),
                context("Failed de_changes deserialization", |input| {
                    self.de_changes_deserializer.deserialize(input)
                }),
                context(
                    "Failed execution_trail_hash_change deserialization",
                    |input| {
                        self.execution_trail_hash_change_deserializer
                            .deserialize(input)
                    },
                ),
            )),
        )
        .map(
            |(
                ledger_changes,
                async_pool_changes,
                deferred_call_changes,
                pos_changes,
                executed_ops_changes,
                executed_denunciations_changes,
                execution_trail_hash_change,
            )| StateChanges {
                ledger_changes,
                async_pool_changes,
                deferred_call_changes,
                pos_changes,
                executed_ops_changes,
                executed_denunciations_changes,
                execution_trail_hash_change,
            },
        )
        .parse(buffer)
    }
}

impl StateChanges {
    /// extends the current `StateChanges` with another one
    pub fn apply(&mut self, changes: StateChanges) {
        use massa_ledger_exports::Applicable;
        self.ledger_changes.apply(changes.ledger_changes);
        self.async_pool_changes.apply(changes.async_pool_changes);
        self.pos_changes.extend(changes.pos_changes);
        self.executed_ops_changes
            .extend(changes.executed_ops_changes);
        self.execution_trail_hash_change
            .apply(changes.execution_trail_hash_change);
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;
    use std::str::FromStr;

    use massa_async_pool::AsyncMessage;
    use massa_ledger_exports::{LedgerEntryUpdate, SetUpdateOrDelete};
    use massa_models::amount::Amount;
    use massa_models::bytecode::Bytecode;
    use massa_models::slot::Slot;
    use massa_models::{address::Address, config::DEFERRED_CALL_MAX_POOL_CHANGES};
    use massa_serialization::DeserializeError;

    use massa_models::config::{
        ENDORSEMENT_COUNT, MAX_BOOTSTRAP_ASYNC_POOL_CHANGES, MAX_DATASTORE_ENTRY_COUNT,
        MAX_DATASTORE_KEY_LENGTH, MAX_DATASTORE_VALUE_LENGTH, MAX_DEFERRED_CREDITS_LENGTH,
        MAX_DENUNCIATION_CHANGES_LENGTH, MAX_EXECUTED_OPS_CHANGES_LENGTH, MAX_FUNCTION_NAME_LENGTH,
        MAX_LEDGER_CHANGES_COUNT, MAX_PARAMETERS_SIZE, MAX_PRODUCTION_STATS_LENGTH,
        MAX_ROLLS_COUNT_LENGTH, THREAD_COUNT,
    };

    use super::*;

    impl PartialEq<StateChanges> for StateChanges {
        fn eq(&self, other: &StateChanges) -> bool {
            self.ledger_changes == other.ledger_changes &&
                self.async_pool_changes == other.async_pool_changes &&
                // pos_changes
                self.pos_changes.seed_bits == other.pos_changes.seed_bits &&
                self.pos_changes.roll_changes == other.pos_changes.roll_changes &&
                self.pos_changes.production_stats == other.pos_changes.production_stats &&
                self.pos_changes.deferred_credits.credits == other.pos_changes.deferred_credits.credits &&
                self.executed_ops_changes == other.executed_ops_changes &&
                self.executed_denunciations_changes == other.executed_denunciations_changes &&
                self.execution_trail_hash_change == other.execution_trail_hash_change
        }
    }

    #[test]
    fn test_state_changes_ser_der() {
        let mut state_changes = StateChanges::default();
        let message = AsyncMessage::new(
            Slot::new(1, 0),
            0,
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
            String::from("test"),
            10000000,
            Amount::from_str("1").unwrap(),
            Amount::from_str("1").unwrap(),
            Slot::new(2, 0),
            Slot::new(3, 0),
            vec![1, 2, 3, 4],
            None,
            None,
        );
        let mut async_pool_changes = AsyncPoolChanges::default();
        async_pool_changes
            .0
            .insert(message.compute_id(), SetUpdateOrDelete::Set(message));
        state_changes.async_pool_changes = async_pool_changes;

        let amount = Amount::from_str("1").unwrap();
        let bytecode = Bytecode(vec![1, 2, 3]);
        let ledger_entry = LedgerEntryUpdate {
            balance: SetOrKeep::Set(amount),
            bytecode: SetOrKeep::Set(bytecode),
            datastore: BTreeMap::default(),
        };
        let mut ledger_changes = LedgerChanges::default();
        ledger_changes.0.insert(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            SetUpdateOrDelete::Update(ledger_entry),
        );
        state_changes.ledger_changes = ledger_changes;
        let mut serialized = Vec::new();
        StateChangesSerializer::new()
            .serialize(&state_changes, &mut serialized)
            .unwrap();

        let (rest, state_changes_deser) = StateChangesDeserializer::new(
            THREAD_COUNT,
            MAX_BOOTSTRAP_ASYNC_POOL_CHANGES,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE as u64,
            MAX_LEDGER_CHANGES_COUNT,
            MAX_DATASTORE_KEY_LENGTH,
            MAX_DATASTORE_VALUE_LENGTH,
            MAX_DATASTORE_ENTRY_COUNT,
            MAX_ROLLS_COUNT_LENGTH,
            MAX_PRODUCTION_STATS_LENGTH,
            MAX_DEFERRED_CREDITS_LENGTH,
            MAX_EXECUTED_OPS_CHANGES_LENGTH,
            ENDORSEMENT_COUNT,
            MAX_DENUNCIATION_CHANGES_LENGTH,
            DEFERRED_CALL_MAX_POOL_CHANGES,
        )
        .deserialize::<DeserializeError>(&serialized)
        .unwrap();
        assert!(rest.is_empty());
        assert_eq!(state_changes, state_changes_deser);
    }

    #[test]
    fn test_state_changes_serde() {
        let mut state_changes = StateChanges::default();
        let message = AsyncMessage::new(
            Slot::new(1, 0),
            0,
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
            String::from("test"),
            10000000,
            Amount::from_str("1").unwrap(),
            Amount::from_str("1").unwrap(),
            Slot::new(2, 0),
            Slot::new(3, 0),
            vec![1, 2, 3, 4],
            None,
            None,
        );
        let mut async_pool_changes = AsyncPoolChanges::default();
        async_pool_changes
            .0
            .insert(message.compute_id(), SetUpdateOrDelete::Set(message));
        state_changes.async_pool_changes = async_pool_changes;

        let amount = Amount::from_str("1").unwrap();
        let bytecode = Bytecode(vec![1, 2, 3]);
        let mut datastore = BTreeMap::new();
        datastore.insert(
            b"hello".to_vec(),
            massa_ledger_exports::SetOrDelete::Set(b"world".to_vec()),
        );
        let ledger_entry = LedgerEntryUpdate {
            balance: SetOrKeep::Set(amount),
            bytecode: SetOrKeep::Set(bytecode),
            datastore,
        };
        let mut ledger_changes = LedgerChanges::default();
        ledger_changes.0.insert(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            SetUpdateOrDelete::Update(ledger_entry),
        );
        state_changes.ledger_changes = ledger_changes;
        let serialized = serde_json::to_string(&state_changes).unwrap();
        let state_changes_deser = serde_json::from_str(&serialized).unwrap();
        assert_eq!(state_changes, state_changes_deser);
    }
}
