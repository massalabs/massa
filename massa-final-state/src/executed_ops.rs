//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed operations.
//! Used to detect operation reuse.

use massa_hash::{Hash, HashDeserializer, HashSerializer, HASH_SIZE_BYTES};
use massa_models::{
    operation::{OperationId, OperationIdDeserializer},
    prehash::PreHashMap,
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
use std::ops::Bound::{Excluded, Included};

const EXECUTED_OPS_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

/// A structure to list and prune previously executed operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutedOps {
    /// Map of the executed operations
    ops: PreHashMap<OperationId, Slot>,
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
            ops: PreHashMap::default(),
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
        for (op_id, slot) in other.ops {
            if self.ops.try_insert(op_id, slot).is_ok() {
                let hash =
                    Hash::compute_from(&[&op_id.to_bytes()[..], &slot.to_bytes_key()[..]].concat());
                self.hash ^= hash;
            }
        }
    }

    /// check if an operation was executed
    pub fn contains(&self, op_id: &OperationId) -> bool {
        self.ops.contains_key(op_id)
    }

    /// marks an op as executed
    pub fn insert(&mut self, op_id: OperationId, expiration_slot: Slot) {
        if self.ops.try_insert(op_id, expiration_slot).is_ok() {
            let hash = Hash::compute_from(
                &[&op_id.to_bytes()[..], &expiration_slot.to_bytes_key()[..]].concat(),
            );
            self.hash ^= hash;
        }
    }

    /// Prune all operations that expire strictly before `max_slot`
    pub fn prune(&mut self, max_slot: Slot) {
        // TODO use slot-sorted structure for more efficient pruning (this has a linear complexity currently)
        let (kept, removed): (PreHashMap<OperationId, Slot>, PreHashMap<OperationId, Slot>) = self
            .ops
            .iter()
            .partition(|(_, &last_valid_slot)| last_valid_slot >= max_slot);
        for (op_id, slot) in removed {
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
        cursor: StreamingStep<OperationId>,
    ) -> (ExecutedOps, StreamingStep<OperationId>) {
        // FOLLOW-UP TODO: stream in multiple parts
        match cursor {
            StreamingStep::Started => (), // TODO: when parts start at unbounded left range
            StreamingStep::Ongoing(_op_id) => (), // TODO: when parts start at op_id left range
            StreamingStep::Finished => return (ExecutedOps::default(), cursor),
        }
        (self.clone(), StreamingStep::Finished)
    }

    /// Set a part of the executed operations.
    ///
    /// Solely used by the bootstrap.
    ///
    /// # Returns
    /// The next executed ops streaming step
    pub fn set_executed_ops_part(&mut self, part: ExecutedOps) -> StreamingStep<OperationId> {
        self.extend(part);
        StreamingStep::Finished
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
                OperationId::new(Hash::compute_from(&[i])),
                Slot {
                    period: i as u64,
                    thread: 0,
                },
            );
        }
        if i > 8 {
            b.insert(
                OperationId::new(Hash::compute_from(&[i])),
                Slot {
                    period: (i as u64),
                    thread: 0,
                },
            );
        }
        c.insert(
            OperationId::new(Hash::compute_from(&[i])),
            Slot {
                period: (i as u64),
                thread: 0,
            },
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
        for (op_id, slot) in &value.ops {
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
                        "Failed peration info deserialization",
                        tuple((
                            |input| self.operation_id_deserializer.deserialize(input),
                            |input| self.slot_deserializer.deserialize(input),
                        )),
                    ),
                ),
                context("Failed hash deserialization", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(elements, hash)| ExecutedOps {
            ops: elements.into_iter().collect(),
            hash,
        })
        .parse(buffer)
    }
}
