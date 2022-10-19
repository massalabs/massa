//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed operations.
//! Used to detect operation reuse.

use massa_hash::{Hash, HashDeserializer, HASH_SIZE_BYTES};
use massa_models::{
    operation::{OperationId, OperationIdDeserializer},
    prehash::PreHashSet,
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
    collections::VecDeque,
    ops::Bound::{Excluded, Included, Unbounded},
};

const EXECUTED_OPS_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

/// A structure to list and prune previously executed operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutedOps {
    /// Executed operations deque associated with a Slot for better pruning complexity
    ops_deque: VecDeque<(Slot, PreHashSet<OperationId>)>,
    /// Executed operations only for better insertion complexity
    ops: PreHashSet<OperationId>,
    /// Accumulated hash of the executed operations
    pub hash: Hash,
    /// Number of threads
    thread_count: u8,
    /// Maximum size of a bootstrap part
    bootstrap_part_size: u64,
}

impl ExecutedOps {
    /// Creates a new `ExecutedOps`
    pub fn new(thread_count: u8, bootstrap_part_size: u64) -> Self {
        Self {
            ops_deque: VecDeque::new(),
            ops: PreHashSet::default(),
            hash: Hash::from_bytes(EXECUTED_OPS_INITIAL_BYTES),
            thread_count,
            bootstrap_part_size,
        }
    }

    /// Returns the number of executed operations
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Check executed ops emptiness
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Internal function used to insert the values of an operation id iter and update the object hash
    fn extend_and_compute_hash<'a, I>(&mut self, values: I)
    where
        I: Iterator<Item = &'a OperationId>,
    {
        for op_id in values {
            if self.ops.insert(*op_id) {
                self.hash ^= Hash::compute_from(op_id.to_bytes());
            }
        }
    }

    /// Apply speculative operations changes to the final executed operations state
    pub fn apply_changes(&mut self, other: PreHashSet<OperationId>, slot: Slot) {
        self.extend_and_compute_hash(other.iter());
        match self.ops_deque.back_mut() {
            Some((last_slot, ids)) if *last_slot == slot => {
                ids.extend(other);
            }
            Some((last_slot, _)) => match last_slot.get_next_slot(self.thread_count) {
                Ok(next_to_last_slot) if next_to_last_slot == slot => {
                    self.ops_deque.push_back((next_to_last_slot, other));
                }
                _ => panic!("executed ops associated slot must be sequential"),
            },
            None => {
                self.ops_deque.push_back((slot, other));
            }
        }
    }

    /// Check if an operation was executed
    pub fn contains(&self, op_id: &OperationId) -> bool {
        self.ops.contains(op_id)
    }

    /// Prune all operations that expire strictly before `slot`
    pub fn prune(&mut self, slot: Slot) {
        let index = match self
            .ops_deque
            .binary_search_by_key(&slot, |(slot, _)| *slot)
        {
            Ok(index) => index,
            Err(_) => return,
        };
        let kept = self.ops_deque.split_off(index);
        let removed = std::mem::take(&mut self.ops_deque);
        for (_, ids) in removed {
            for op_id in ids {
                self.ops.remove(&op_id);
                self.hash ^= Hash::compute_from(op_id.to_bytes());
            }
        }
        self.ops_deque = kept;
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
    ) -> (
        VecDeque<(Slot, PreHashSet<OperationId>)>,
        StreamingStep<Slot>,
    ) {
        let mut ops_part = VecDeque::new();
        let left_bound = match cursor {
            StreamingStep::Started => Unbounded,
            StreamingStep::Ongoing(slot) => {
                match self
                    .ops_deque
                    .binary_search_by_key(&slot, |(slot, _)| *slot)
                {
                    Ok(index) => Excluded(index),
                    Err(_) => return (ops_part, StreamingStep::Finished),
                }
            }
            StreamingStep::Finished => return (ops_part, cursor),
        };
        let mut ops_part_last_slot: Option<Slot> = None;
        for (slot, ids) in self.ops_deque.range((left_bound, Unbounded)) {
            if self.ops_deque.len() < self.bootstrap_part_size as usize {
                ops_part.push_back((*slot, *ids));
                ops_part_last_slot = Some(*slot);
            } else {
                break;
            }
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
    pub fn set_executed_ops_part(
        &mut self,
        part: VecDeque<(Slot, PreHashSet<OperationId>)>,
    ) -> StreamingStep<Slot> {
        if let Some(slot) = self.ops_deque.back().map(|(slot, _)| slot) {
            self.ops_deque.extend(part);
            self.extend_and_compute_hash(part.iter().flat_map(|(slot, ids)| ids));
            StreamingStep::Ongoing(*slot)
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
        }
    }
}

impl Serializer<VecDeque<(Slot, PreHashSet<OperationId>)>> for ExecutedOpsSerializer {
    fn serialize(
        &self,
        value: &VecDeque<(Slot, PreHashSet<OperationId>)>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        // executed ops length
        self.u64_serializer
            .serialize(&(value.len() as u64), buffer)?;
        // executed ops
        for (slot, ids) in value {
            // slot
            self.slot_serializer.serialize(slot, buffer)?;
            // slot ids length
            self.u64_serializer.serialize(&(ids.len() as u64), buffer)?;
            // slots ids
            for op_id in ids {
                buffer.extend(op_id.to_bytes());
            }
        }
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

impl Deserializer<VecDeque<(Slot, PreHashSet<OperationId>)>> for ExecutedOpsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], VecDeque<(Slot, PreHashSet<OperationId>)>, E> {
        context(
            "ExecutedOps",
            length_count(
                context("ExecutedOps length", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                context(
                    "slot operations",
                    tuple((
                        context("slot", |input| self.slot_deserializer.deserialize(input)),
                        length_count(
                            context("slot operations length", |input| {
                                self.u64_deserializer.deserialize(input)
                            }),
                            context("operation id", |input| {
                                self.operation_id_deserializer.deserialize(input)
                            }),
                        ),
                    )),
                ),
            ),
        )
        .map(|operations| {
            operations
                .into_iter()
                .map(|(slot, ids)| (slot, ids.into_iter().collect()))
                .collect()
        })
        .parse(buffer)
    }
}
