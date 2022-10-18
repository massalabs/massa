//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed operations.
//! Used to detect operation reuse.

use massa_hash::{Hash, HashDeserializer, HashSerializer, HASH_SIZE_BYTES};
use massa_models::{
    operation::{OperationId, OperationIdDeserializer},
    slot::{Slot, SlotDeserializer, SlotSerializer},
    streaming_step::StreamingStep,
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
use std::{
    collections::BTreeMap,
    ops::Bound::{Excluded, Included, Unbounded},
};

const EXECUTED_OPS_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

/// A structure to list and prune previously executed operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutedOps {
    /// Map of the executed operations
    ops: BTreeMap<Slot, OperationId>,
    /// Accumulated hash of the executed operations
    pub hash: Hash,
}

impl Default for ExecutedOps {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutedOps {
    /// Creates a new `ExecutedOps`
    pub fn new() -> Self {
        Self {
            ops: BTreeMap::new(),
            hash: Hash::from_bytes(EXECUTED_OPS_INITIAL_BYTES),
        }
    }

    /// returns the number of executed operations
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Check is there is no executed ops
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// extends with another `ExecutedOps`
    pub fn extend(&mut self, other: ExecutedOps) {
        for (slot, op_id) in other.ops {
            if self.ops.try_insert(slot, op_id).is_ok() {
                let hash =
                    Hash::compute_from(&[&op_id.to_bytes()[..], &slot.to_bytes_key()[..]].concat());
                self.hash ^= hash;
            }
        }
    }

    /// check if an operation was executed
    pub fn contains(&self, op_id: &OperationId) -> bool {
        self.ops.values().any(|contained_id| contained_id == op_id)
    }

    /// marks an op as executed
    pub fn insert(&mut self, expiration_slot: Slot, op_id: OperationId) {
        if self.ops.try_insert(expiration_slot, op_id).is_ok() {
            let hash = Hash::compute_from(
                &[&op_id.to_bytes()[..], &expiration_slot.to_bytes_key()[..]].concat(),
            );
            self.hash ^= hash;
        }
    }

    /// Prune all operations that expire strictly before `slot`
    pub fn prune(&mut self, slot: Slot) {
        let kept = self.ops.split_off(&slot);
        let removed = std::mem::take(&mut self.ops);
        for (slot, op_id) in removed {
            let hash =
                Hash::compute_from(&[&op_id.to_bytes()[..], &slot.to_bytes_key()[..]].concat());
            self.hash ^= hash;
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
        cursor: StreamingStep<Slot>,
    ) -> (ExecutedOps, StreamingStep<Slot>) {
        let mut ops_part = ExecutedOps::default();
        let left_bound = match cursor {
            StreamingStep::Started => Unbounded,
            StreamingStep::Ongoing(operation_id) => Excluded(operation_id),
            StreamingStep::Finished => return (ops_part, cursor),
        };
        let mut ops_part_last_slot: Option<Slot> = None;
        for (&slot, &op_id) in self.ops.range((left_bound, Unbounded)) {
            // FOLLOW-UP TODO: stream in multiple parts
            ops_part.insert(slot, op_id);
            ops_part_last_slot = Some(slot);
        }
        if let Some(last_slot) = ops_part_last_slot {
            (ops_part, StreamingStep::Ongoing(last_slot))
        } else {
            (ops_part, StreamingStep::Finished)
        }
    }

    /// Set a part of the executed operations.
    ///
    /// Solely used by the bootstrap.
    ///
    /// # Returns
    /// The next executed ops streaming step
    pub fn set_executed_ops_part(&mut self, part: ExecutedOps) -> StreamingStep<Slot> {
        self.extend(part);
        if let Some(slot) = self.ops.last_key_value().map(|(&slot, _)| slot) {
            StreamingStep::Ongoing(slot)
        } else {
            StreamingStep::Finished
        }
    }
}

#[test]
fn test_executed_ops_xor_computing() {
    use massa_models::wrapped::Id;
    let mut a = ExecutedOps::default();
    let mut b = ExecutedOps::default();
    let mut c = ExecutedOps::default();
    // initialize the three different objects
    for i in 0u8..20 {
        if i < 12 {
            a.insert(
                Slot {
                    period: i as u64,
                    thread: 0,
                },
                OperationId::new(Hash::compute_from(&[i])),
            );
        }
        if i > 8 {
            b.insert(
                Slot {
                    period: (i as u64),
                    thread: 0,
                },
                OperationId::new(Hash::compute_from(&[i])),
            );
        }
        c.insert(
            Slot {
                period: (i as u64),
                thread: 0,
            },
            OperationId::new(Hash::compute_from(&[i])),
        );
    }
    // extend a with b which performs a.hash ^ b.hash
    a.extend(b);
    // check that a.hash ^ b.hash = c.hash
    assert_eq!(a.hash, c.hash);
    // prune every element
    a.prune(Slot {
        period: 20,
        thread: 0,
    });
    // at this point the hash should have been XORed with itself
    assert_eq!(a.hash, Hash::from_bytes(EXECUTED_OPS_INITIAL_BYTES));
}

/// `ExecutedOps` Serializer
pub struct ExecutedOpsSerializer {
    slot_serializer: SlotSerializer,
    u64_serializer: U64VarIntSerializer,
    hash_serializer: HashSerializer,
}

impl Default for ExecutedOpsSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutedOpsSerializer {
    /// Create a new `ExecutedOps` Serializer
    pub fn new() -> ExecutedOpsSerializer {
        ExecutedOpsSerializer {
            slot_serializer: SlotSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
            hash_serializer: HashSerializer::new(),
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
        for (slot, op_id) in &value.ops {
            buffer.extend(op_id.to_bytes());
            self.slot_serializer.serialize(slot, buffer)?;
        }

        // encode the hash
        self.hash_serializer.serialize(&value.hash, buffer)?;
        Ok(())
    }
}

/// Deserializer for `ExecutedOps`
pub struct ExecutedOpsDeserializer {
    operation_id_deserializer: OperationIdDeserializer,
    slot_deserializer: SlotDeserializer,
    u64_deserializer: U64VarIntDeserializer,
    hash_deserializer: HashDeserializer,
}

impl ExecutedOpsDeserializer {
    /// Create a new deserializer for `ExecutedOps`
    pub fn new(thread_count: u8) -> ExecutedOpsDeserializer {
        ExecutedOpsDeserializer {
            operation_id_deserializer: OperationIdDeserializer::new(),
            slot_deserializer: SlotDeserializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            hash_deserializer: HashDeserializer::new(),
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
            tuple((
                length_count(
                    context("Failed length deserialization", |input| {
                        self.u64_deserializer.deserialize(input)
                    }),
                    context(
                        "Failed operation info deserialization",
                        tuple((
                            |input| self.slot_deserializer.deserialize(input),
                            |input| self.operation_id_deserializer.deserialize(input),
                        )),
                    ),
                ),
                context("Failed hash deserialization", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(operations, hash)| ExecutedOps {
            ops: operations.into_iter().collect(),
            hash,
        })
        .parse(buffer)
    }
}
