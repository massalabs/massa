//! Copyright (c) 2023 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed denunciations.
//! Used to detect denunciation reuse.

use std::collections::{BTreeMap, HashSet};
use std::ops::Bound::{Excluded, Included, Unbounded};

use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};

use crate::{
    config::ExecutedDenunciationsConfig, denunciations_changes::ExecutedDenunciationsChanges,
};

use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_models::denunciation::Denunciation;
use massa_models::streaming_step::StreamingStep;
use massa_models::{
    denunciation::{DenunciationIndex, DenunciationIndexDeserializer, DenunciationIndexSerializer},
    slot::{Slot, SlotDeserializer, SlotSerializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};

const EXECUTED_DENUNCIATIONS_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

/// A structure to list and prune previously executed denunciations
#[derive(Debug, Clone)]
pub struct ExecutedDenunciations {
    /// Executed denunciations configuration
    config: ExecutedDenunciationsConfig,
    /// for better pruning complexity
    pub sorted_denunciations: BTreeMap<Slot, HashSet<DenunciationIndex>>,
    /// for better insertion complexity
    pub(crate) denunciations: HashSet<DenunciationIndex>,
    /// Accumulated hash of the executed denunciations
    pub hash: Hash,
}

impl ExecutedDenunciations {
    /// Create a new `ExecutedDenunciations`
    pub fn new(config: ExecutedDenunciationsConfig) -> Self {
        Self {
            config,
            sorted_denunciations: Default::default(),
            denunciations: Default::default(),
            hash: Hash::from_bytes(EXECUTED_DENUNCIATIONS_HASH_INITIAL_BYTES),
        }
    }

    /// Reset the executed operations
    ///
    /// USED FOR BOOTSTRAP ONLY
    pub fn reset(&mut self) {
        self.sorted_denunciations.clear();
        self.denunciations.clear();
        self.hash = Hash::from_bytes(EXECUTED_DENUNCIATIONS_HASH_INITIAL_BYTES);
    }

    /// Returns the number of executed operations
    pub(crate) fn len(&self) -> usize {
        self.denunciations.len()
    }

    /// Check executed ops emptiness
    pub(crate) fn is_empty(&self) -> bool {
        self.denunciations.is_empty()
    }

    /// Check if a denunciation (e.g. a denunciation index) was executed
    pub(crate) fn contains(&self, de_idx: &DenunciationIndex) -> bool {
        self.denunciations.contains(de_idx)
    }

    /// Internal function used to insert the values of an operation id iter and update the object hash
    fn extend_and_compute_hash<'a, I>(&mut self, values: I)
    where
        I: Iterator<Item = &'a DenunciationIndex>,
    {
        for de_idx in values {
            if self.denunciations.insert((*de_idx).clone()) {
                self.hash ^= de_idx.get_hash();
            }
        }
    }

    /// Apply speculative operations changes to the final executed denunciations state
    pub fn apply_changes(&mut self, changes: ExecutedDenunciationsChanges, slot: Slot) {
        self.extend_and_compute_hash(changes.iter());
        for de_idx in changes {
            self.sorted_denunciations
                .entry(*de_idx.get_slot())
                .and_modify(|ids| {
                    ids.insert(de_idx.clone());
                })
                .or_insert_with(|| {
                    let mut new = HashSet::default();
                    new.insert(de_idx.clone());
                    new
                });
        }

        self.prune(slot);
    }

    /// Prune all denunciations that have expired, assuming the given slot is final
    fn prune(&mut self, slot: Slot) {
        let drained: Vec<(Slot, HashSet<DenunciationIndex>)> = self
            .sorted_denunciations
            .drain_filter(|de_idx_slot, _| {
                Denunciation::is_expired(
                    &de_idx_slot.period,
                    &slot.period,
                    &self.config.denunciation_expire_periods,
                )
            })
            .collect();

        for (_slot, de_indexes) in drained {
            for de_idx in de_indexes {
                self.denunciations.remove(&de_idx);
                self.hash ^= de_idx.get_hash();
            }
        }
    }

    /// Get a part of the executed denunciations.
    /// Used exclusively by the bootstrap server.
    ///
    /// # Returns
    /// A tuple containing the data and the next executed de streaming step
    pub(crate) fn get_executed_de_part(
        &self,
        cursor: StreamingStep<Slot>,
    ) -> (
        BTreeMap<Slot, HashSet<DenunciationIndex>>,
        StreamingStep<Slot>,
    ) {
        let mut de_part = BTreeMap::new();
        let left_bound = match cursor {
            StreamingStep::Started => Unbounded,
            StreamingStep::Ongoing(slot) => Excluded(slot),
            StreamingStep::Finished(_) => return (de_part, cursor),
        };
        let mut de_part_last_slot: Option<Slot> = None;
        for (slot, ids) in self.sorted_denunciations.range((left_bound, Unbounded)) {
            if de_part.len() < self.config.bootstrap_part_size as usize {
                de_part.insert(*slot, ids.clone());
                de_part_last_slot = Some(*slot);
            } else {
                break;
            }
        }
        if let Some(last_slot) = de_part_last_slot {
            (de_part, StreamingStep::Ongoing(last_slot))
        } else {
            (de_part, StreamingStep::Finished(None))
        }
    }

    /// Set a part of the executed denunciations.
    /// Used exclusively by the bootstrap client.
    /// Takes the data returned from `get_executed_de_part` as input.
    ///
    /// # Returns
    /// The next executed de streaming step
    pub(crate) fn set_executed_de_part(
        &mut self,
        part: BTreeMap<Slot, HashSet<DenunciationIndex>>,
    ) -> StreamingStep<Slot> {
        self.sorted_denunciations.extend(part.clone());
        self.extend_and_compute_hash(part.iter().flat_map(|(_, ids)| ids));
        if let Some(slot) = self
            .sorted_denunciations
            .last_key_value()
            .map(|(slot, _)| slot)
        {
            StreamingStep::Ongoing(*slot)
        } else {
            StreamingStep::Finished(None)
        }
    }
}

/// `ExecutedDenunciations` Serializer
pub struct ExecutedDenunciationsSerializer {
    slot_serializer: SlotSerializer,
    de_idx_serializer: DenunciationIndexSerializer,
    u64_serializer: U64VarIntSerializer,
}

impl Default for ExecutedDenunciationsSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutedDenunciationsSerializer {
    /// Create a new `ExecutedDenunciations` Serializer
    pub fn new() -> Self {
        Self {
            slot_serializer: SlotSerializer::new(),
            de_idx_serializer: DenunciationIndexSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
        }
    }
}

impl Serializer<BTreeMap<Slot, HashSet<DenunciationIndex>>> for ExecutedDenunciationsSerializer {
    fn serialize(
        &self,
        value: &BTreeMap<Slot, HashSet<DenunciationIndex>>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        // exec denunciations length
        self.u64_serializer
            .serialize(&(value.len() as u64), buffer)?;
        // executed ops
        for (slot, ids) in value {
            // slot
            self.slot_serializer.serialize(slot, buffer)?;
            // slot ids length
            self.u64_serializer.serialize(&(ids.len() as u64), buffer)?;
            // slots ids
            for de_idx in ids {
                self.de_idx_serializer.serialize(de_idx, buffer)?;
            }
        }
        Ok(())
    }
}

/// Deserializer for `ExecutedDenunciations`
pub struct ExecutedDenunciationsDeserializer {
    de_idx_deserializer: DenunciationIndexDeserializer,
    slot_deserializer: SlotDeserializer,
    ops_length_deserializer: U64VarIntDeserializer,
    slot_ops_length_deserializer: U64VarIntDeserializer,
}

impl ExecutedDenunciationsDeserializer {
    /// Create a new deserializer for `ExecutedDenunciations`
    pub fn new(
        thread_count: u8,
        endorsement_count: u32,
        max_executed_de_length: u64,
        max_denunciations_per_block_header: u64,
    ) -> Self {
        Self {
            de_idx_deserializer: DenunciationIndexDeserializer::new(
                thread_count,
                endorsement_count,
            ),
            slot_deserializer: SlotDeserializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            ops_length_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_executed_de_length),
            ),
            slot_ops_length_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_denunciations_per_block_header),
            ),
        }
    }
}

impl Deserializer<BTreeMap<Slot, HashSet<DenunciationIndex>>>
    for ExecutedDenunciationsDeserializer
{
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BTreeMap<Slot, HashSet<DenunciationIndex>>, E> {
        context(
            "ExecutedDenunciations",
            length_count(
                context("length", |input| {
                    self.ops_length_deserializer.deserialize(input)
                }),
                context(
                    "slot de_idx",
                    tuple((
                        context("slot", |input| self.slot_deserializer.deserialize(input)),
                        length_count(
                            context("slot denunciations length", |input| {
                                self.slot_ops_length_deserializer.deserialize(input)
                            }),
                            context("denunciation index", |input| {
                                self.de_idx_deserializer.deserialize(input)
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
