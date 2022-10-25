//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file provides structures representing changes to the final state

use massa_async_pool::{
    AsyncPoolChanges, AsyncPoolChangesDeserializer, AsyncPoolChangesSerializer,
};
use massa_ledger_exports::{LedgerChanges, LedgerChangesDeserializer, LedgerChangesSerializer};
use massa_pos_exports::{PoSChanges, PoSChangesDeserializer, PoSChangesSerializer};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};

use crate::{executed_ops::ExecutedOpsSerializer, ExecutedOps, ExecutedOpsDeserializer};

/// represents changes that can be applied to the execution state
#[derive(Default, Debug, Clone)]
pub struct StateChanges {
    /// ledger changes
    pub ledger_changes: LedgerChanges,
    /// asynchronous pool changes
    pub async_pool_changes: AsyncPoolChanges,
    /// roll state changes
    pub pos_changes: PoSChanges,
    /// executed operations: maps the operation ID to its validity slot end - included
    pub executed_ops: ExecutedOps,
}

/// Basic `StateChanges` serializer.
pub struct StateChangesSerializer {
    ledger_changes_serializer: LedgerChangesSerializer,
    async_pool_changes_serializer: AsyncPoolChangesSerializer,
    pos_changes_serializer: PoSChangesSerializer,
    executed_ops_serializer: ExecutedOpsSerializer,
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
            pos_changes_serializer: PoSChangesSerializer::new(),
            executed_ops_serializer: ExecutedOpsSerializer::new(),
        }
    }
}

impl Serializer<StateChanges> for StateChangesSerializer {
    /// ## Example
    /// ```
    /// use massa_serialization::Serializer;
    /// use massa_models::{address::Address, amount::Amount, slot::Slot};
    /// use massa_final_state::{StateChanges, StateChangesSerializer};
    /// use std::str::FromStr;
    /// use std::collections::BTreeMap;
    /// use massa_ledger_exports::{LedgerEntryUpdate, SetOrKeep, SetUpdateOrDelete, LedgerChanges};
    /// use massa_async_pool::{AsyncMessage, Change, AsyncPoolChanges};
    ///
    /// let mut state_changes = StateChanges::default();
    /// let message = AsyncMessage {
    ///     emission_slot: Slot::new(1, 0),
    ///     emission_index: 0,
    ///     sender:  Address::from_str("A12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///     destination: Address::from_str("A12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
    ///     handler: String::from("test"),
    ///     max_gas: 10000000,
    ///     gas_price: Amount::from_str("1").unwrap(),
    ///     coins: Amount::from_str("1").unwrap(),
    ///     validity_start: Slot::new(2, 0),
    ///     validity_end: Slot::new(3, 0),
    ///     data: vec![1, 2, 3, 4]
    /// };
    /// let async_pool_changes: AsyncPoolChanges = AsyncPoolChanges(vec![Change::Add(message.compute_id(), message)]);
    /// state_changes.async_pool_changes = async_pool_changes;
    ///
    /// let amount = Amount::from_str("1").unwrap();
    /// let bytecode = vec![1, 2, 3];
    /// let ledger_entry = LedgerEntryUpdate {
    ///    balance: SetOrKeep::Set(amount),
    ///    bytecode: SetOrKeep::Set(bytecode),
    ///    datastore: BTreeMap::default(),
    /// };
    /// let mut ledger_changes = LedgerChanges::default();
    /// ledger_changes.0.insert(
    ///    Address::from_str("A12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///    SetUpdateOrDelete::Update(ledger_entry),
    /// );
    /// state_changes.ledger_changes = ledger_changes;
    /// let mut serialized = Vec::new();
    /// StateChangesSerializer::new().serialize(&state_changes, &mut serialized).unwrap();
    /// ```
    fn serialize(&self, value: &StateChanges, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.ledger_changes_serializer
            .serialize(&value.ledger_changes, buffer)?;
        self.async_pool_changes_serializer
            .serialize(&value.async_pool_changes, buffer)?;
        self.pos_changes_serializer
            .serialize(&value.pos_changes, buffer)?;
        self.executed_ops_serializer
            .serialize(&value.executed_ops, buffer)?;
        Ok(())
    }
}

/// Basic `StateChanges` deserializer
pub struct StateChangesDeserializer {
    ledger_changes_deserializer: LedgerChangesDeserializer,
    async_pool_changes_deserializer: AsyncPoolChangesDeserializer,
    pos_changes_deserializer: PoSChangesDeserializer,
    executed_ops_deserializer: ExecutedOpsDeserializer,
}

impl StateChangesDeserializer {
    /// Creates a `StateChangesDeserializer`
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        thread_count: u8,
        max_async_pool_changes: u64,
        max_async_message_data: u64,
        max_ledger_changes_count: u64,
        max_datastore_key_length: u8,
        max_datastore_value_length: u64,
        max_datastore_entry_count: u64,
        max_rolls_length: u64,
        max_production_stats_length: u64,
        max_credits_length: u64,
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
                max_async_message_data,
            ),
            pos_changes_deserializer: PoSChangesDeserializer::new(
                thread_count,
                max_rolls_length,
                max_production_stats_length,
                max_credits_length,
            ),
            executed_ops_deserializer: ExecutedOpsDeserializer::new(thread_count),
        }
    }
}

impl Deserializer<StateChanges> for StateChangesDeserializer {
    /// ## Example
    /// ```
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_models::{address::Address, prehash::PreHashMap, amount::Amount, slot::Slot};
    /// use massa_final_state::{StateChanges, StateChangesSerializer, StateChangesDeserializer};
    /// use std::str::FromStr;
    /// use std::collections::BTreeMap;
    /// use massa_ledger_exports::{LedgerEntryUpdate, SetOrKeep, SetUpdateOrDelete, LedgerChanges};
    /// use massa_async_pool::{AsyncMessage, Change, AsyncPoolChanges};
    ///
    /// let mut state_changes = StateChanges::default();
    /// let message = AsyncMessage {
    ///     emission_slot: Slot::new(1, 0),
    ///     emission_index: 0,
    ///     sender:  Address::from_str("A12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///     destination: Address::from_str("A12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
    ///     handler: String::from("test"),
    ///     max_gas: 10000000,
    ///     gas_price: Amount::from_str("1").unwrap(),
    ///     coins: Amount::from_str("1").unwrap(),
    ///     validity_start: Slot::new(2, 0),
    ///     validity_end: Slot::new(3, 0),
    ///     data: vec![1, 2, 3, 4]
    /// };
    /// let async_pool_changes: AsyncPoolChanges = AsyncPoolChanges(vec![Change::Add(message.compute_id(), message)]);
    /// state_changes.async_pool_changes = async_pool_changes;
    ///
    /// let amount = Amount::from_str("1").unwrap();
    /// let bytecode = vec![1, 2, 3];
    /// let ledger_entry = LedgerEntryUpdate {
    ///    balance: SetOrKeep::Set(amount),
    ///    bytecode: SetOrKeep::Set(bytecode),
    ///    datastore: BTreeMap::default(),
    /// };
    /// let mut ledger_changes = LedgerChanges::default();
    /// ledger_changes.0.insert(
    ///    Address::from_str("A12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///    SetUpdateOrDelete::Update(ledger_entry),
    /// );
    /// state_changes.ledger_changes = ledger_changes;
    /// let mut serialized = Vec::new();
    /// StateChangesSerializer::new().serialize(&state_changes, &mut serialized).unwrap();
    /// let (rest, state_changes_deser) = StateChangesDeserializer::new(32, 255, 255, 255, 255, 255, 255, 255, 255, 255).deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(state_changes_deser.ledger_changes, state_changes.ledger_changes);
    /// assert_eq!(state_changes_deser.async_pool_changes, state_changes.async_pool_changes);
    /// ```
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
                context("Failed roll_state_changes deserialization", |input| {
                    self.pos_changes_deserializer.deserialize(input)
                }),
                context("Failed executed_ops deserialization", |input| {
                    self.executed_ops_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(ledger_changes, async_pool_changes, roll_state_changes, executed_ops)| StateChanges {
                ledger_changes,
                async_pool_changes,
                pos_changes: roll_state_changes,
                executed_ops,
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
        self.async_pool_changes.extend(changes.async_pool_changes);
        self.pos_changes.extend(changes.pos_changes);
        self.executed_ops.extend(changes.executed_ops);
    }
}
