//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed operations.
//! Used to detect operation reuse.

use massa_models::{
    prehash::Map, OperationId, OperationIdDeserializer, Slot, SlotDeserializer, SlotSerializer,
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

/// A structure to list and prune previously executed operations
#[derive(Debug, Default, Clone)]
pub struct ExecutedOps(Map<OperationId, Slot>);

impl ExecutedOps {
    /// extends with another ExecutedOps
    pub fn extend(&mut self, other: ExecutedOps) {
        self.0.extend(other.0);
    }

    /// check if an operation was executed
    pub fn contains(&self, op_id: &OperationId) -> bool {
        self.0.contains_key(op_id)
    }

    /// marks an op as executed
    pub fn insert(&mut self, op_id: OperationId, last_valid_slot: Slot) {
        self.0.insert(op_id, last_valid_slot);
    }

    /// Prune all operations that expire strictly before max_slot
    pub fn prune(&mut self, max_slot: Slot) {
        // TODO maybe use slot-sorted structure for more efficient pruning (this has a linear complexity currently)
        self.0
            .retain(|_id, last_valid_slot| *last_valid_slot >= max_slot);
    }
}

/// `ExecutedOps` Serializer
pub struct ExecutedOpsSerializer {
    slot_serializer: SlotSerializer,
    u64_serializer: U64VarIntSerializer,
}

impl ExecutedOpsSerializer {
    /// Create a new `ExecutedOps` Serializer
    pub fn new(thread_count: u8) -> ExecutedOpsSerializer {
        ExecutedOpsSerializer {
            slot_serializer: SlotSerializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            u64_serializer: U64VarIntSerializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Serializer<ExecutedOps> for ExecutedOpsSerializer {
    fn serialize(&self, value: &ExecutedOps, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // encode the number of entries
        let entry_count: u64 = value.0.len().try_into().map_err(|err| {
            SerializeError::GeneralError(format!("too many entries in ExecutedOps: {}", err))
        })?;
        self.u64_serializer.serialize(&entry_count, buffer)?;

        // encode entries
        for (op_id, slot) in &value.0 {
            buffer.extend(op_id.to_bytes());
            self.slot_serializer.serialize(&slot, buffer)?;
        }

        Ok(())
    }
}

/// Deserializer for ExecutedOps
pub struct ExecutedOpsDeserializer {
    operation_id_deserializer: OperationIdDeserializer,
    slot_deserializer: SlotDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl ExecutedOpsDeserializer {
    /// Create a new deserializer for ExecutedOps
    pub fn new(thread_count: u8) -> ExecutedOpsDeserializer {
        ExecutedOpsDeserializer {
            operation_id_deserializer: OperationIdDeserializer::new(),
            slot_deserializer: SlotDeserializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Deserializer<ExecutedOps> for ExecutedOpsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ExecutedOps, E> {
        context(
            "Failed ExecutedOps deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                tuple((
                    |input| self.operation_id_deserializer.deserialize(input),
                    |input| self.slot_deserializer.deserialize(input),
                )),
            ),
        )
        .map(|elements| ExecutedOps(elements.into_iter().collect()))
        .parse(buffer)
    }
}
