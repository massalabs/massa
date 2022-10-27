//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    operation::{OperationId, OperationIdDeserializer, OperationIdSerializer},
    prehash::PreHashMap,
    slot::{Slot, SlotDeserializer, SlotSerializer},
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
use std::ops::Bound::{Excluded, Included};

/// Speculatives changes for ExecutedOps
pub type ExecutedOpsChanges = PreHashMap<OperationId, Slot>;

/// `ExecutedOps` Serializer
pub struct ExecutedOpsChangesSerializer {
    u64_serializer: U64VarIntSerializer,
    operation_id_serializer: OperationIdSerializer,
    slot_serializer: SlotSerializer,
}

impl Default for ExecutedOpsChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutedOpsChangesSerializer {
    /// Create a new `ExecutedOps` Serializer
    pub fn new() -> ExecutedOpsChangesSerializer {
        ExecutedOpsChangesSerializer {
            u64_serializer: U64VarIntSerializer::new(),
            operation_id_serializer: OperationIdSerializer::new(),
            slot_serializer: SlotSerializer::new(),
        }
    }
}

impl Serializer<ExecutedOpsChanges> for ExecutedOpsChangesSerializer {
    fn serialize(
        &self,
        value: &ExecutedOpsChanges,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.u64_serializer
            .serialize(&(value.len() as u64), buffer)?;
        for (op_id, slot) in value {
            self.operation_id_serializer.serialize(op_id, buffer)?;
            self.slot_serializer.serialize(slot, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `ExecutedOps`
pub struct ExecutedOpsChangesDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    operation_id_deserializer: OperationIdDeserializer,
    slot_deserializer: SlotDeserializer,
}

impl ExecutedOpsChangesDeserializer {
    /// Create a new deserializer for `ExecutedOps`
    pub fn new(thread_count: u8, max_ops_changes_length: u64) -> ExecutedOpsChangesDeserializer {
        ExecutedOpsChangesDeserializer {
            u64_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_ops_changes_length),
            ),
            operation_id_deserializer: OperationIdDeserializer::new(),
            slot_deserializer: SlotDeserializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
        }
    }
}

impl Deserializer<ExecutedOpsChanges> for ExecutedOpsChangesDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ExecutedOpsChanges, E> {
        context(
            "ExecutedOpsChanges",
            length_count(
                context("ExecutedOpsChanges length", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                tuple((
                    context("operation id", |input| {
                        self.operation_id_deserializer.deserialize(input)
                    }),
                    context("expiration slot", |input| {
                        self.slot_deserializer.deserialize(input)
                    }),
                )),
            ),
        )
        .map(|ids| ids.into_iter().collect())
        .parse(buffer)
    }
}
