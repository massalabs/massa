//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed operations.
//! Used to detect operation reuse.

use crate::{ops_changes::ExecutedOpsChanges, ExecutedOpsConfig};
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_models::{
    operation::{OperationId, OperationIdDeserializer},
    prehash::PreHashSet,
    secure_share::Id,
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
    collections::{BTreeMap, HashMap},
    ops::Bound::{Excluded, Included, Unbounded},
};

const EXECUTED_OPS_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

/// A structure to list and prune previously executed operations
#[derive(Debug, Clone)]
pub struct ExecutedOps {
    /// Executed operations configuration
    config: ExecutedOpsConfig,
    /// Executed operations btreemap with slot as index for better pruning complexity
    pub sorted_ops: BTreeMap<Slot, PreHashSet<OperationId>>,
    /// Executed operations only for better insertion complexity
    pub ops: PreHashSet<OperationId>,
    /// Accumulated hash of the executed operations
    pub hash: Hash,
    /// execution status of operations (true: success, false: fail)
    pub op_exec_status: HashMap<OperationId, bool>,
}

impl ExecutedOps {
    /// Creates a new `ExecutedOps`
    pub fn new(config: ExecutedOpsConfig) -> Self {
        Self {
            config,
            sorted_ops: BTreeMap::new(),
            ops: PreHashSet::default(),
            hash: Hash::from_bytes(EXECUTED_OPS_HASH_INITIAL_BYTES),
            op_exec_status: HashMap::new(),
        }
    }

    /// Reset the executed operations
    ///
    /// USED FOR BOOTSTRAP ONLY
    pub fn reset(&mut self) {
        self.sorted_ops.clear();
        self.ops.clear();
        self.hash = Hash::from_bytes(EXECUTED_OPS_HASH_INITIAL_BYTES);
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
                self.hash ^= *op_id.get_hash();
            }
        }
    }

    /// Apply speculative operations changes to the final executed operations state
    pub fn apply_changes(&mut self, changes: ExecutedOpsChanges, slot: Slot) {
        self.extend_and_compute_hash(changes.keys());
        for (op_id, (op_exec_success, slot)) in changes {
            self.sorted_ops
                .entry(slot)
                .and_modify(|ids| {
                    ids.insert(op_id);
                })
                .or_insert_with(|| {
                    let mut new = PreHashSet::default();
                    new.insert(op_id);
                    new
                });

            self.op_exec_status.insert(op_id, op_exec_success);
        }

        self.prune(slot);
    }

    /// Check if an operation was executed
    pub fn contains(&self, op_id: &OperationId) -> bool {
        self.ops.contains(op_id)
    }

    /// Prune all operations that expire strictly before `slot`
    fn prune(&mut self, slot: Slot) {
        let kept = self.sorted_ops.split_off(&slot);
        let removed = std::mem::take(&mut self.sorted_ops);
        for (_, ids) in removed {
            for op_id in ids {
                self.ops.remove(&op_id);
                self.op_exec_status.remove(&op_id);
                self.hash ^= *op_id.get_hash();
            }
        }
        self.sorted_ops = kept;
    }

    /// Get a part of the executed operations.
    /// Used exclusively by the bootstrap server.
    ///
    /// # Returns
    /// A tuple containing the data and the next executed ops streaming step
    pub fn get_executed_ops_part(
        &self,
        cursor: StreamingStep<Slot>,
    ) -> (BTreeMap<Slot, PreHashSet<OperationId>>, StreamingStep<Slot>) {
        let mut ops_part = BTreeMap::new();
        let left_bound = match cursor {
            StreamingStep::Started => Unbounded,
            StreamingStep::Ongoing(slot) => Excluded(slot),
            StreamingStep::Finished(_) => return (ops_part, cursor),
        };
        let mut ops_part_last_slot: Option<Slot> = None;
        for (slot, ids) in self.sorted_ops.range((left_bound, Unbounded)) {
            if ops_part.len() < self.config.bootstrap_part_size as usize {
                ops_part.insert(*slot, ids.clone());
                ops_part_last_slot = Some(*slot);
            } else {
                break;
            }
        }
        if let Some(last_slot) = ops_part_last_slot {
            (ops_part, StreamingStep::Ongoing(last_slot))
        } else {
            (ops_part, StreamingStep::Finished(None))
        }
    }

    /// Set a part of the executed operations.
    /// Used exclusively by the bootstrap client.
    /// Takes the data returned from `get_executed_ops_part` as input.
    ///
    /// # Returns
    /// The next executed ops streaming step
    pub fn set_executed_ops_part(
        &mut self,
        part: BTreeMap<Slot, PreHashSet<OperationId>>,
    ) -> StreamingStep<Slot> {
        self.sorted_ops.extend(part.clone());
        self.extend_and_compute_hash(part.iter().flat_map(|(_, ids)| ids));
        if let Some(slot) = self.sorted_ops.last_key_value().map(|(slot, _)| slot) {
            StreamingStep::Ongoing(*slot)
        } else {
            StreamingStep::Finished(None)
        }
    }
}

#[test]
fn test_executed_ops_xor_computing() {
    use massa_models::prehash::PreHashMap;
    use massa_models::secure_share::Id;

    // initialize the executed ops config
    let config = ExecutedOpsConfig {
        thread_count: 2,
        bootstrap_part_size: 10,
    };

    // initialize the executed ops and executed ops changes
    let mut a = ExecutedOps::new(config.clone());
    let mut c = ExecutedOps::new(config);
    let mut change_a = PreHashMap::default();
    let mut change_b = PreHashMap::default();
    let mut change_c = PreHashMap::default();
    for i in 0u8..20 {
        let expiration_slot = Slot {
            period: i as u64,
            thread: 0,
        };
        if i < 12 {
            change_a.insert(
                OperationId::new(Hash::compute_from(&[i])),
                (true, expiration_slot),
            );
        }
        if i > 8 {
            change_b.insert(
                OperationId::new(Hash::compute_from(&[i])),
                (true, expiration_slot),
            );
        }
        change_c.insert(
            OperationId::new(Hash::compute_from(&[i])),
            (true, expiration_slot),
        );
    }

    // apply change_b to a which performs a.hash ^ $(change_b)
    let apply_slot = Slot {
        period: 0,
        thread: 0,
    };
    a.apply_changes(change_a, apply_slot);
    a.apply_changes(change_b, apply_slot);
    c.apply_changes(change_c, apply_slot);

    // check that a.hash ^ $(change_b) = c.hash
    assert_eq!(a.hash, c.hash, "'a' and 'c' hashes are not equal");

    // prune every element
    let prune_slot = Slot {
        period: 20,
        thread: 0,
    };
    a.apply_changes(PreHashMap::default(), prune_slot);
    a.prune(prune_slot);

    // at this point the hash should have been XORed with itself
    assert_eq!(
        a.hash,
        Hash::from_bytes(EXECUTED_OPS_HASH_INITIAL_BYTES),
        "'a' was not reset to its initial value"
    );
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

impl Serializer<BTreeMap<Slot, PreHashSet<OperationId>>> for ExecutedOpsSerializer {
    fn serialize(
        &self,
        value: &BTreeMap<Slot, PreHashSet<OperationId>>,
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

impl Deserializer<BTreeMap<Slot, PreHashSet<OperationId>>> for ExecutedOpsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BTreeMap<Slot, PreHashSet<OperationId>>, E> {
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
