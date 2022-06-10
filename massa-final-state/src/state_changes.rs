//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file provides structures representing changes to the final state

use massa_async_pool::{
    AsyncPoolChanges, AsyncPoolChangesDeserializer, AsyncPoolChangesSerializer,
};
use massa_ledger_exports::{LedgerChanges, LedgerChangesDeserializer, LedgerChangesSerializer};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::{error::context, sequence::tuple, IResult};

/// represents changes that can be applied to the execution state
#[derive(Default, Debug, Clone)]
pub struct StateChanges {
    /// ledger changes
    pub ledger_changes: LedgerChanges,
    /// asynchronous pool changes
    pub async_pool_changes: AsyncPoolChanges,
}

/// Basic `StateChanges` serializer.
pub struct StateChangesSerializer {
    ledger_changes_serializer: LedgerChangesSerializer,
    async_pool_changes_serializer: AsyncPoolChangesSerializer,
}

impl StateChangesSerializer {
    /// Creates a `StateChangesSerializer`
    pub fn new() -> Self {
        Self {
            ledger_changes_serializer: LedgerChangesSerializer::new(),
            async_pool_changes_serializer: AsyncPoolChangesSerializer::new(),
        }
    }
}

impl Default for StateChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<StateChanges> for StateChangesSerializer {
    fn serialize(&self, value: &StateChanges) -> Result<Vec<u8>, SerializeError> {
        let ledger_changes = self
            .ledger_changes_serializer
            .serialize(&value.ledger_changes)?;
        let async_pool_changes = self
            .async_pool_changes_serializer
            .serialize(&value.async_pool_changes)?;
        let mut res = Vec::with_capacity(ledger_changes.len() + async_pool_changes.len());
        res.extend(ledger_changes);
        res.extend(async_pool_changes);
        Ok(res)
    }
}

/// Basic `StateChanges` deserializer
pub struct StateChangesDeserializer {
    ledger_changes_deserializer: LedgerChangesDeserializer,
    async_pool_changes_deserializer: AsyncPoolChangesDeserializer,
}

impl StateChangesDeserializer {
    /// Creates a `StateChangesDeserializer`
    pub fn new() -> Self {
        Self {
            ledger_changes_deserializer: LedgerChangesDeserializer::new(),
            async_pool_changes_deserializer: AsyncPoolChangesDeserializer::new(),
        }
    }
}

impl Default for StateChangesDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<StateChanges> for StateChangesDeserializer {
    fn deserialize<'a>(&self, buffer: &'a [u8]) -> IResult<&'a [u8], StateChanges> {
        let mut parser = tuple((
            context("ledger changes in state changes", |input| {
                self.ledger_changes_deserializer.deserialize(input)
            }),
            context("async pool changes in state changes", |input| {
                self.async_pool_changes_deserializer.deserialize(input)
            }),
        ));
        parser(buffer).map(|(rest, (ledger_changes, async_pool_changes))| {
            (
                rest,
                StateChanges {
                    ledger_changes,
                    async_pool_changes,
                },
            )
        })
    }
}

impl StateChanges {
    /// extends the current `StateChanges` with another one
    pub fn apply(&mut self, changes: StateChanges) {
        use massa_ledger_exports::Applicable;
        self.ledger_changes.apply(changes.ledger_changes);
        self.async_pool_changes.extend(changes.async_pool_changes);
    }
}
