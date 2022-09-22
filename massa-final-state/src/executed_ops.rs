//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed operations.
//! Used to detect operation reuse.

use massa_hash::Hash;
use massa_models::{
    error::ModelsError,
    operation::{OperationId, OperationIdDeserializer},
    prehash::PreHashMap,
    slot::{Slot, SlotDeserializer, SlotSerializer},
    wrapped::Id,
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
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExecutedOps {
    ops: PreHashMap<OperationId, Slot>,
    hash: Option<Hash>,
}

impl ExecutedOps {
    /// returns the number of executed operations
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Check is there is no executed ops
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// extends with another ExecutedOps
    pub fn extend(&mut self, other: ExecutedOps) {
        self.ops.extend(other.ops);
    }

    /// check if an operation was executed
    pub fn contains(&self, op_id: &OperationId) -> bool {
        self.ops.contains_key(op_id)
    }

    /// marks an op as executed
    pub fn insert(&mut self, op_id: OperationId, last_valid_slot: Slot) {
        if let Some(hash) = self.hash.as_mut() {
            *hash ^= *op_id.get_hash();
        } else {
            self.hash = Some(*op_id.get_hash());
        }
        self.ops.insert(op_id, last_valid_slot);
    }

    /// Prune all operations that expire strictly before max_slot
    pub fn prune(&mut self, max_slot: Slot) {
        // TODO use slot-sorted structure for more efficient pruning (this has a linear complexity currently)
        let (kept, removed): (PreHashMap<OperationId, Slot>, PreHashMap<OperationId, Slot>) = self
            .ops
            .iter()
            .partition(|(_, &last_valid_slot)| last_valid_slot >= max_slot);
        let hash = self
            .hash
            .as_mut()
            .expect("critical: an ExecutedOps object with ops must also contain a hash");
        for (op_id, _) in removed {
            *hash ^= *op_id.get_hash();
        }
        self.ops = kept;
    }

    /// Get a part of the executed operations.
    ///
    /// Solely used by the bootstrap.
    ///
    /// # Returns
    /// A tuple containing the data and the next executed ops streaming step
    pub fn get_executed_ops_part(
        &self,
        cursor: ExecutedOpsStreamingStep,
    ) -> Result<(Vec<u8>, ExecutedOpsStreamingStep), ModelsError> {
        // TODO: stream in multiple parts
        match cursor {
            ExecutedOpsStreamingStep::Started => (), // TODO: when parts start at unbounded left range
            ExecutedOpsStreamingStep::Ongoing(_op_id) => (), // TODO: when parts start at op_id left range
            ExecutedOpsStreamingStep::Finished => {
                return Ok((Vec::new(), ExecutedOpsStreamingStep::Finished))
            }
        }
        let mut part = Vec::new();
        let ops_serializer = ExecutedOpsSerializer::new();
        ops_serializer.serialize(self, &mut part)?;
        Ok((part, ExecutedOpsStreamingStep::Finished))
    }

    /// Set a part of the executed operations.
    ///
    /// Solely used by the bootstrap.
    ///
    /// # Returns
    /// The next executed ops streaming step
    pub fn set_executed_ops_part(
        &mut self,
        part: &[u8],
        thread_count: u8,
    ) -> Result<ExecutedOpsStreamingStep, ModelsError> {
        if part.is_empty() {
            return Ok(ExecutedOpsStreamingStep::Finished);
        }
        let ops_deserializer = ExecutedOpsDeserializer::new(thread_count);
        let (rest, ops) = ops_deserializer.deserialize(part)?;
        if !rest.is_empty() {
            return Err(ModelsError::SerializeError(
                "data is left after set_executed_ops_part deserialization".to_string(),
            ));
        }
        self.extend(ops);
        Ok(ExecutedOpsStreamingStep::Finished)
    }
}

/// Executed operations bootstrap streaming steps
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum ExecutedOpsStreamingStep {
    /// Started step, only when launching the streaming
    Started,
    /// Ongoing step, as long as there are operations to stream
    Ongoing(OperationId),
    /// Finished step, after the last operations where streamed
    Finished,
}

/// Executed operations bootstrap streaming steps serializer
#[derive(Default)]
pub struct ExecutedOpsStreamingStepSerializer {
    u64_serializer: U64VarIntSerializer,
}

impl ExecutedOpsStreamingStepSerializer {
    /// Creates a new executed operations bootstrap streaming steps serializer
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer,
        }
    }
}

impl Serializer<ExecutedOpsStreamingStep> for ExecutedOpsStreamingStepSerializer {
    fn serialize(
        &self,
        value: &ExecutedOpsStreamingStep,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        match value {
            ExecutedOpsStreamingStep::Started => self.u64_serializer.serialize(&0u64, buffer)?,
            ExecutedOpsStreamingStep::Ongoing(op_id) => {
                self.u64_serializer.serialize(&1u64, buffer)?;
                buffer.extend(op_id.to_bytes());
            }
            ExecutedOpsStreamingStep::Finished => self.u64_serializer.serialize(&2u64, buffer)?,
        };
        Ok(())
    }
}

/// Executed operations bootstrap streaming steps deserializer
pub struct ExecutedOpsStreamingStepDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    op_id_deserializer: OperationIdDeserializer,
}

impl Default for ExecutedOpsStreamingStepDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutedOpsStreamingStepDeserializer {
    /// Creates a new executed operations bootstrap streaming steps deserializer
    pub fn new() -> Self {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            op_id_deserializer: OperationIdDeserializer::new(),
        }
    }
}

impl Deserializer<ExecutedOpsStreamingStep> for ExecutedOpsStreamingStepDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ExecutedOpsStreamingStep, E> {
        let (rest, ident) = context("identifier", |input| {
            self.u64_deserializer.deserialize(input)
        })
        .parse(buffer)?;
        match ident {
            0u64 => Ok((rest, ExecutedOpsStreamingStep::Started)),
            1u64 => context("operation_id", |input| {
                self.op_id_deserializer.deserialize(input)
            })
            .map(ExecutedOpsStreamingStep::Ongoing)
            .parse(rest),

            2u64 => Ok((rest, ExecutedOpsStreamingStep::Finished)),
            _ => Err(nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Digit,
            ))),
        }
    }
}

/// `ExecutedOps` Serializer
#[derive(Default)]
pub struct ExecutedOpsSerializer {
    slot_serializer: SlotSerializer,
    u64_serializer: U64VarIntSerializer,
}

impl ExecutedOpsSerializer {
    /// Create a new `ExecutedOps` Serializer
    pub fn new() -> ExecutedOpsSerializer {
        ExecutedOpsSerializer {
            slot_serializer: SlotSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
        }
    }
}

impl Serializer<ExecutedOps> for ExecutedOpsSerializer {
    fn serialize(&self, value: &ExecutedOps, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // encode the number of entries
        let entry_count: u64 = value.ops.len().try_into().map_err(|err| {
            SerializeError::GeneralError(format!("too many entries in ExecutedOps: {}", err))
        })?;
        self.u64_serializer.serialize(&entry_count, buffer)?;

        // encode entries
        for (op_id, slot) in &value.ops {
            buffer.extend(op_id.to_bytes());
            self.slot_serializer.serialize(slot, buffer)?;
        }

        // TODO: ser hash
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
        .map(|elements| ExecutedOps {
            ops: elements.into_iter().collect(),
            // TODO: deser hash
            hash: None,
        })
        .parse(buffer)
    }
}
