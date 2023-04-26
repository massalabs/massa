//! Copyright (c) 2023 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously processed denunciations.
//! Used to detect denunciation reuse.

use std::collections::{BTreeMap, HashSet};
use std::ops::Bound::{Excluded, Included, Unbounded};
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
    write_batch, DBBatch, CF_ERROR, CRUD_ERROR, EXECUTED_DENUNCIATIONS_CF,
    EXECUTED_DENUNCIATIONS_HASH_ERROR, EXECUTED_DENUNCIATIONS_HASH_INITIAL_BYTES,
    EXECUTED_DENUNCIATIONS_HASH_KEY, EXECUTED_DENUNCIATIONS_INDEX_DESER_ERROR,
    EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR, METADATA_CF, WRONG_BATCH_TYPE_ERROR,
};
use massa_hash::Hash;
use massa_models::streaming_step::StreamingStep;
use massa_models::{
    denunciation::{DenunciationIndex, DenunciationIndexDeserializer, DenunciationIndexSerializer},
    slot::{Slot, SlotDeserializer, SlotSerializer},
};
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};
use rocksdb::{IteratorMode, Options, DB};

/// A structure to list and prune previously processed denunciations
#[derive(Clone)]
pub struct ExecutedDenunciations {
    /// Processed denunciations configuration
    config: ExecutedDenunciationsConfig,
    /// Access to the RocksDB database
    pub db: Arc<RwLock<DB>>,
    /// for better pruning complexity
    pub sorted_denunciations: BTreeMap<Slot, HashSet<DenunciationIndex>>,
    denunciation_index_serializer: DenunciationIndexSerializer,
    denunciation_index_deserializer: DenunciationIndexDeserializer,
}

impl ExecutedDenunciations {
    /// Create a new `ProcessedDenunciations`
    pub fn new(config: ExecutedDenunciationsConfig, db: Arc<RwLock<DB>>) -> Self {
        let denunciation_index_deserializer =
            DenunciationIndexDeserializer::new(config.thread_count, config.endorsement_count);
        Self {
            config,
            db,
            sorted_denunciations: Default::default(),
            denunciation_index_serializer: DenunciationIndexSerializer::new(),
            denunciation_index_deserializer,
        }
    }

    fn recompute_sorted_denunciations(&mut self) {
        self.sorted_denunciations.clear();

        let db = self.db.read();
        let handle = db.cf_handle(EXECUTED_DENUNCIATIONS_CF).expect(CF_ERROR);

        for (serialized_de_idx, _) in db.iterator_cf(handle, IteratorMode::Start).flatten() {
            let (_, de_idx) = self
                .denunciation_index_deserializer
                .deserialize::<DeserializeError>(&serialized_de_idx)
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
            (*db)
                .drop_cf(EXECUTED_DENUNCIATIONS_CF)
                .expect("Error dropping executed_denunciations cf");
            let mut db_opts = Options::default();
            db_opts.set_error_if_exists(true);
            (*db)
                .create_cf(EXECUTED_DENUNCIATIONS_CF, &db_opts)
                .expect("Error creating executed_denunciations cf");
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

    /// Check if a denunciation (e.g. a denunciation index) was processed
    pub fn contains(&self, de_idx: &DenunciationIndex) -> bool {
        let db = self.db.read();
        let handle = db.cf_handle(EXECUTED_DENUNCIATIONS_CF).expect(CF_ERROR);

        let mut serialized_de_idx = Vec::new();
        self.denunciation_index_serializer
            .serialize(de_idx, &mut serialized_de_idx)
            .expect(EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR);

        db.get_cf(handle, &serialized_de_idx)
            .expect(CRUD_ERROR)
            .is_some()
    }

    /// Apply speculative operations changes to the final processed denunciations state
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
                slot.period.checked_sub(de_idx_slot.period)
                    > Some(self.config.denunciation_expire_periods)
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
    /// * `message_id`
    /// * `message`
    /// * `batch`: the given operation batch to update
    fn put_entry(&self, de_idx: &DenunciationIndex, batch: &mut DBBatch) {
        let db = self.db.read();
        let handle = db.cf_handle(EXECUTED_DENUNCIATIONS_CF).expect(CF_ERROR);

        let mut serialized_de_idx = Vec::new();
        self.denunciation_index_serializer
            .serialize(de_idx, &mut serialized_de_idx)
            .expect(EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR);

        if db
            .get_cf(handle, &serialized_de_idx)
            .expect(CRUD_ERROR)
            .is_none()
        {
            let hash = de_idx.get_hash();
            batch.aeh_list.insert(serialized_de_idx.clone(), hash);
            *batch
                .executed_denunciations_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^= hash;
            batch.write_batch.put_cf(handle, serialized_de_idx, "");
        }
    }

    /// Remove a denunciation_index from the DB
    ///
    /// # Arguments
    /// * batch: the given operation batch to update
    fn delete_entry(&self, de_idx: &DenunciationIndex, batch: &mut DBBatch) {
        let db = self.db.read();
        let handle = db.cf_handle(EXECUTED_DENUNCIATIONS_CF).expect(CF_ERROR);

        let mut serialized_de_idx = Vec::new();
        self.denunciation_index_serializer
            .serialize(de_idx, &mut serialized_de_idx)
            .expect(EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR);

        if let Some(added_hash) = batch.aeh_list.get(&serialized_de_idx) {
            *batch
                .executed_denunciations_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^= *added_hash;
        } else if db
            .get_pinned_cf(handle, &serialized_de_idx)
            .expect(CRUD_ERROR)
            .is_some()
        {
            let hash = de_idx.get_hash();
            *batch
                .executed_denunciations_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^= hash
        }
        batch.write_batch.delete_cf(handle, serialized_de_idx);
    }

    /// Get a part of the processed denunciations.
    /// Used exclusively by the bootstrap server.
    ///
    /// # Returns
    /// A tuple containing the data and the next executed de streaming step
    pub fn get_processed_de_part(
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

    /// Set a part of the processed denunciations.
    /// Used exclusively by the bootstrap client.
    /// Takes the data returned from `get_processed_de_part` as input.
    ///
    /// # Returns
    /// The next executed de streaming step
    pub fn set_processed_de_part(
        &mut self,
        part: BTreeMap<Slot, HashSet<DenunciationIndex>>,
    ) -> StreamingStep<Slot> {
        let mut batch = DBBatch::new(None, None, None, None, None, Some(self.get_hash()));

        self.sorted_denunciations.extend(part.clone());

        for de_idx in part.iter().flat_map(|(_, ids)| ids) {
            self.put_entry(de_idx, &mut batch);
        }

        write_batch(&self.db.read(), batch);

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

    /// Get the current executed denunciations hash
    pub fn get_hash(&self) -> Hash {
        let db = self.db.read();
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);
        if let Some(executed_denunciations_hash) = db
            .get_cf(handle, EXECUTED_DENUNCIATIONS_HASH_KEY)
            .expect(CRUD_ERROR)
            .as_deref()
        {
            Hash::from_bytes(
                executed_denunciations_hash
                    .try_into()
                    .expect(EXECUTED_DENUNCIATIONS_HASH_ERROR),
            )
        } else {
            // initial executed_denunciations value to avoid matching an option in every XOR operation
            // because of a one time case being an empty ledger
            // also note that the if you XOR a hash with itself result is EXECUTED_DENUNCIATIONS_HASH_INITIAL_BYTES
            Hash::from_bytes(EXECUTED_DENUNCIATIONS_HASH_INITIAL_BYTES)
        }
    }
}

/// `ProcessedDenunciations` Serializer
pub struct ProcessedDenunciationsSerializer {
    slot_serializer: SlotSerializer,
    de_idx_serializer: DenunciationIndexSerializer,
    u64_serializer: U64VarIntSerializer,
}

impl Default for ProcessedDenunciationsSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessedDenunciationsSerializer {
    /// Create a new `ProcessedDenunciations` Serializer
    pub fn new() -> Self {
        Self {
            slot_serializer: SlotSerializer::new(),
            de_idx_serializer: DenunciationIndexSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
        }
    }
}

impl Serializer<BTreeMap<Slot, HashSet<DenunciationIndex>>> for ProcessedDenunciationsSerializer {
    fn serialize(
        &self,
        value: &BTreeMap<Slot, HashSet<DenunciationIndex>>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        // processed denunciations length
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

/// Deserializer for `ProcessedDenunciations`
pub struct ProcessedDenunciationsDeserializer {
    de_idx_deserializer: DenunciationIndexDeserializer,
    slot_deserializer: SlotDeserializer,
    ops_length_deserializer: U64VarIntDeserializer,
    slot_ops_length_deserializer: U64VarIntDeserializer,
}

impl ProcessedDenunciationsDeserializer {
    /// Create a new deserializer for `ProcessedDenunciations`
    pub fn new(
        thread_count: u8,
        endorsement_count: u32,
        max_processed_de_length: u64,
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
                Included(max_processed_de_length),
            ),
            slot_ops_length_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_denunciations_per_block_header),
            ),
        }
    }
}

impl Deserializer<BTreeMap<Slot, HashSet<DenunciationIndex>>>
    for ProcessedDenunciationsDeserializer
{
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BTreeMap<Slot, HashSet<DenunciationIndex>>, E> {
        context(
            "ProcessedDenunciations",
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
