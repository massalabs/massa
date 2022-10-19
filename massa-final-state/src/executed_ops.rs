//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed operations.
//! Used to detect operation reuse.

use crate::ops_changes::ExecutedOpsChanges;
use massa_hash::{Hash, HASH_SIZE_BYTES};
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
    pub fn apply_changes(&mut self, changes: ExecutedOpsChanges, slot: Slot) {
        self.extend_and_compute_hash(changes.iter());
        match self.ops_deque.back_mut() {
            Some((last_slot, ids)) if *last_slot == slot => {
                ids.extend(changes);
            }
            Some((last_slot, _)) => match last_slot.get_next_slot(self.thread_count) {
                Ok(next_to_last_slot) if next_to_last_slot == slot => {
                    self.ops_deque.push_back((next_to_last_slot, changes));
                }
                _ => panic!("executed ops associated slot must be sequential"),
            },
            None => {
                self.ops_deque.push_back((slot, changes));
            }
        }
        self.prune(slot);
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
                ops_part.push_back((*slot, ids.clone()));
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
        self.ops_deque.extend(part.clone());
        self.extend_and_compute_hash(part.iter().flat_map(|(_, ids)| ids));
        if let Some(slot) = self.ops_deque.back().map(|(slot, _)| slot) {
            StreamingStep::Ongoing(*slot)
        } else {
            StreamingStep::Finished
        }
    }
}

#[test]
fn test_executed_ops_xor_computing() {
    use massa_models::prehash::PreHashSet;
    use massa_models::wrapped::Id;

    // initialize the changes and the executed ops objects
    let mut change_a = PreHashSet::default();
    let mut change_b = PreHashSet::default();
    let mut change_c = PreHashSet::default();
    for i in 0u8..20 {
        if i < 12 {
            change_a.insert(OperationId::new(Hash::compute_from(&[i])));
        }
        if i > 8 {
            change_b.insert(OperationId::new(Hash::compute_from(&[i])));
        }
        change_c.insert(OperationId::new(Hash::compute_from(&[i])));
    }
    let slot = Slot {
        period: 20,
        thread: 0,
    };
    let mut a = ExecutedOps::new(2, 10);
    let mut c = ExecutedOps::new(2, 10);
    a.apply_changes(change_a, slot);
    c.apply_changes(change_c, slot);

    // extend a with change_b which performs a.hash ^ $(change_b)
    a.extend_and_compute_hash(change_b.iter());
    // check that a.hash ^ $(change_b) = c.hash
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
    ops_length_deserializer: U64VarIntDeserializer,
    slot_ops_length_deserializer: U64VarIntDeserializer,
}

impl ExecutedOpsDeserializer {
    /// Create a new deserializer for `ExecutedOps`
    pub fn new(
        thread_count: u8,
        max_executed_ops_length: u64,
        max_operations_per_block: u64,
    ) -> ExecutedOpsDeserializer {
        ExecutedOpsDeserializer {
            operation_id_deserializer: OperationIdDeserializer::new(),
            slot_deserializer: SlotDeserializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            ops_length_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_executed_ops_length),
            ),
            slot_ops_length_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_operations_per_block),
            ),
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
                    self.ops_length_deserializer.deserialize(input)
                }),
                context(
                    "slot operations",
                    tuple((
                        context("slot", |input| self.slot_deserializer.deserialize(input)),
                        length_count(
                            context("slot operations length", |input| {
                                self.slot_ops_length_deserializer.deserialize(input)
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
