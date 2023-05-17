//! Copyright (c) 2023 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed denunciations.
//! Used to detect denunciation reuse.

use std::collections::{BTreeMap, HashSet};
use std::ops::Bound::{Excluded, Included};
use std::sync::Arc;

use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use parking_lot::RwLock;

use crate::{ExecutedDenunciationsChanges, ExecutedDenunciationsConfig};
use massa_db::{
    DBBatch, MassaDB, CF_ERROR, CRUD_ERROR, EXECUTED_DENUNCIATIONS_INDEX_DESER_ERROR,
    EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR, EXECUTED_DENUNCIATIONS_PREFIX, STATE_CF,
};
use massa_models::denunciation::Denunciation;
use massa_models::{
    denunciation::{DenunciationIndex, DenunciationIndexDeserializer, DenunciationIndexSerializer},
    slot::{Slot, SlotDeserializer, SlotSerializer},
};
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};

/// Denunciation index key formatting macro
#[macro_export]
macro_rules! denunciation_index_key {
    ($id:expr) => {
        [&EXECUTED_DENUNCIATIONS_PREFIX.as_bytes(), &$id[..]].concat()
    };
}

/// A structure to list and prune previously executed denunciations
#[derive(Clone)]
pub struct ExecutedDenunciations {
    /// Executed denunciations configuration
    config: ExecutedDenunciationsConfig,
    /// Access to the RocksDB database
    pub db: Arc<RwLock<MassaDB>>,
    /// for better pruning complexity
    pub sorted_denunciations: BTreeMap<Slot, HashSet<DenunciationIndex>>,
    /// for rocksdb serialization
    denunciation_index_serializer: DenunciationIndexSerializer,
    /// for rocksdb deserialization
    denunciation_index_deserializer: DenunciationIndexDeserializer,
}

impl ExecutedDenunciations {
    /// Create a new `ExecutedDenunciations`
    pub fn new(config: ExecutedDenunciationsConfig, db: Arc<RwLock<MassaDB>>) -> Self {
        let denunciation_index_deserializer =
            DenunciationIndexDeserializer::new(config.thread_count, config.endorsement_count);
        let mut executed_denunciations = Self {
            config,
            db,
            sorted_denunciations: Default::default(),
            denunciation_index_serializer: DenunciationIndexSerializer::new(),
            denunciation_index_deserializer,
        };
        executed_denunciations.recompute_sorted_denunciations();
        executed_denunciations
    }

    pub fn recompute_sorted_denunciations(&mut self) {
        self.sorted_denunciations.clear();

        let db = self.db.read();
        let handle = db.db.cf_handle(STATE_CF).expect(CF_ERROR);

        for (serialized_de_idx, _) in db
            .db
            .prefix_iterator_cf(handle, EXECUTED_DENUNCIATIONS_PREFIX)
            .flatten()
        {
            if !serialized_de_idx.starts_with(EXECUTED_DENUNCIATIONS_PREFIX.as_bytes()) {
                break;
            }
            let (_, de_idx) = self
                .denunciation_index_deserializer
                .deserialize::<DeserializeError>(
                    &serialized_de_idx[EXECUTED_DENUNCIATIONS_PREFIX.len()..],
                )
                .expect(EXECUTED_DENUNCIATIONS_INDEX_DESER_ERROR);

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
    }

    /// Reset the executed operations
    ///
    /// USED FOR BOOTSTRAP ONLY
    pub fn reset(&mut self) {
        {
            let mut db = self.db.write();
            db.delete_prefix(EXECUTED_DENUNCIATIONS_PREFIX, None);
        }

        self.recompute_sorted_denunciations();
    }

    /*/// Returns the number of executed operations
    pub fn len(&self) -> usize {
        self.denunciations.len()
    }

    /// Check executed ops emptiness
    pub fn is_empty(&self) -> bool {
        self.denunciations.is_empty()
    }*/

    /// Check if a denunciation (e.g. a denunciation index) was executed
    pub fn contains(&self, de_idx: &DenunciationIndex) -> bool {
        let db = self.db.read();
        let handle = db.db.cf_handle(STATE_CF).expect(CF_ERROR);

        let mut serialized_de_idx = Vec::new();
        self.denunciation_index_serializer
            .serialize(de_idx, &mut serialized_de_idx)
            .expect(EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR);

        db.db
            .get_cf(handle, denunciation_index_key!(serialized_de_idx))
            .expect(CRUD_ERROR)
            .is_some()
    }

    /// Apply speculative operations changes to the final executed denunciations state
    pub fn apply_changes_to_batch(
        &mut self,
        changes: ExecutedDenunciationsChanges,
        slot: Slot,
        batch: &mut DBBatch,
    ) {
        for de_idx in changes {
            self.put_entry(&de_idx, batch);
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

        self.prune_to_batch(slot, batch);
    }

    /// Prune all denunciations that have expired, assuming the given slot is final
    fn prune_to_batch(&mut self, slot: Slot, batch: &mut DBBatch) {
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
                self.delete_entry(&de_idx, batch)
            }
        }
    }

    /// Add a denunciation_index to the DB
    ///
    /// # Arguments
    /// * `de_idx`
    /// * `batch`: the given operation batch to update
    fn put_entry(&self, de_idx: &DenunciationIndex, batch: &mut DBBatch) {
        let db = self.db.read();

        let mut serialized_de_idx = Vec::new();
        self.denunciation_index_serializer
            .serialize(de_idx, &mut serialized_de_idx)
            .expect(EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR);

        db.put_or_update_entry_value(batch, denunciation_index_key!(serialized_de_idx), b"");
    }

    /// Remove a denunciation_index from the DB
    ///
    /// # Arguments
    /// * batch: the given operation batch to update
    fn delete_entry(&self, de_idx: &DenunciationIndex, batch: &mut DBBatch) {
        let db = self.db.read();

        let mut serialized_de_idx = Vec::new();
        self.denunciation_index_serializer
            .serialize(de_idx, &mut serialized_de_idx)
            .expect(EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR);

        db.delete_key(batch, denunciation_index_key!(serialized_de_idx));
    }

    /// Deserializes the key and value, useful after bootstrap
    pub fn is_key_value_valid(&self, serialized_key: &[u8], serialized_value: &[u8]) -> bool {
        if !serialized_key.starts_with(EXECUTED_DENUNCIATIONS_PREFIX.as_bytes()) {
            return false;
        }

        let Ok((rest, _idx)) = self.denunciation_index_deserializer.deserialize::<DeserializeError>(&serialized_key[EXECUTED_DENUNCIATIONS_PREFIX.len()..]) else {
            return false;
        };
        if !rest.is_empty() {
            return false;
        }

        if !serialized_value.is_empty() {
            return false;
        }

        true
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
