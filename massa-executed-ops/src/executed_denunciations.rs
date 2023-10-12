//! Copyright (c) 2023 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed denunciations.
//! Used to detect denunciation reuse.

use crate::{ExecutedDenunciationsChanges, ExecutedDenunciationsConfig};
use massa_db_exports::{
    DBBatch, ShareableMassaDBController, CRUD_ERROR, EXECUTED_DENUNCIATIONS_INDEX_DESER_ERROR,
    EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR, EXECUTED_DENUNCIATIONS_PREFIX, STATE_CF,
};
use massa_models::denunciation::Denunciation;
use massa_models::{
    denunciation::{DenunciationIndex, DenunciationIndexDeserializer, DenunciationIndexSerializer},
    slot::Slot,
};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use std::collections::{BTreeMap, HashSet};

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
    pub db: ShareableMassaDBController,
    /// for better pruning complexity
    pub sorted_denunciations: BTreeMap<Slot, HashSet<DenunciationIndex>>,
    /// for rocksdb serialization
    denunciation_index_serializer: DenunciationIndexSerializer,
    /// for rocksdb deserialization
    denunciation_index_deserializer: DenunciationIndexDeserializer,
}

impl ExecutedDenunciations {
    /// Create a new `ExecutedDenunciations`
    pub fn new(config: ExecutedDenunciationsConfig, db: ShareableMassaDBController) -> Self {
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

    /// Recomputes the local caches after bootstrap or loading the state from disk
    pub fn recompute_sorted_denunciations(&mut self) {
        self.sorted_denunciations.clear();

        let db = self.db.read();

        for (serialized_de_idx, _) in
            db.prefix_iterator_cf(STATE_CF, EXECUTED_DENUNCIATIONS_PREFIX.as_bytes())
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
                    ids.insert(de_idx);
                })
                .or_insert_with(|| {
                    let mut new = HashSet::default();
                    new.insert(de_idx);
                    new
                });
        }
    }

    /// Reset the executed denunciations
    ///
    /// USED FOR BOOTSTRAP ONLY
    pub fn reset(&mut self) {
        {
            let mut db = self.db.write();
            db.delete_prefix(EXECUTED_DENUNCIATIONS_PREFIX, STATE_CF, None);
        }

        self.recompute_sorted_denunciations();
    }

    /// Check if a denunciation (e.g. a denunciation index) was executed
    pub fn contains(&self, de_idx: &DenunciationIndex) -> bool {
        let db = self.db.read();

        let mut serialized_de_idx = Vec::new();
        self.denunciation_index_serializer
            .serialize(de_idx, &mut serialized_de_idx)
            .expect(EXECUTED_DENUNCIATIONS_INDEX_SER_ERROR);

        db.get_cf(STATE_CF, denunciation_index_key!(serialized_de_idx))
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
                    ids.insert(de_idx);
                })
                .or_insert_with(|| {
                    let mut new = HashSet::default();
                    new.insert(de_idx);
                    new
                });
        }

        self.prune_to_batch(slot, batch);
    }

    /// Prune all denunciations that have expired, assuming the given slot is final
    fn prune_to_batch(&mut self, slot: Slot, batch: &mut DBBatch) {
        // Force-keep `keep_executed_history_extra_periods` for API polling safety
        let effective_expiry_periods = self
            .config
            .denunciation_expire_periods
            .saturating_add(self.config.keep_executed_history_extra_periods);
        let mut drained: HashSet<DenunciationIndex> = Default::default();
        self.sorted_denunciations.retain(|de_idx_slot, de_idx| {
            if Denunciation::is_expired(
                &de_idx_slot.period,
                &slot.period,
                &effective_expiry_periods,
            ) {
                drained.extend(de_idx.iter());
                return false;
            }
            true
        });
        for de_idx in drained {
            self.delete_entry(&de_idx, batch);
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
    /// * `de_idx`: the denunciation index to remove
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

        let Ok((rest, _idx)) = self
            .denunciation_index_deserializer
            .deserialize::<DeserializeError>(
                &serialized_key[EXECUTED_DENUNCIATIONS_PREFIX.len()..],
            )
        else {
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

#[cfg(test)]
mod test {
    use super::*;
    use massa_db_exports::{MassaDBConfig, MassaDBController};
    use massa_db_worker::MassaDB;
    use massa_models::config::{
        DENUNCIATION_EXPIRE_PERIODS, ENDORSEMENT_COUNT, KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
        THREAD_COUNT,
    };
    use parking_lot::RwLock;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn test_exec_de_cache() {
        // Check executed denunciations cache grow / reset / recompute

        let config = ExecutedDenunciationsConfig {
            denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
            thread_count: THREAD_COUNT,
            endorsement_count: ENDORSEMENT_COUNT,
            keep_executed_history_extra_periods: KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
        };
        // Db init
        let temp_dir = tempdir().expect("Unable to create a temp folder");
        // println!("Using temp dir: {:?}", temp_dir.path());
        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 100,
            max_new_elements: 100,
            thread_count: THREAD_COUNT,
        };
        let db = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config.clone())) as Box<(dyn MassaDBController + 'static)>
        ));

        let mut exec_de = ExecutedDenunciations::new(config.clone(), db);

        let slot_1 = Slot::new(1, 0);
        let de_idx_1 = DenunciationIndex::Endorsement {
            slot: slot_1,
            index: ENDORSEMENT_COUNT - 4,
        };
        let slot_2 = Slot::new(
            DENUNCIATION_EXPIRE_PERIODS + KEEP_EXECUTED_HISTORY_EXTRA_PERIODS + 2,
            3,
        );
        let de_idx_2 = DenunciationIndex::Endorsement {
            slot: slot_2,
            index: ENDORSEMENT_COUNT - 1,
        };
        let mut changes = ExecutedDenunciationsChanges::new();
        changes.insert(de_idx_1.clone());
        changes.insert(de_idx_2.clone());
        let mut batch = DBBatch::new();
        exec_de.apply_changes_to_batch(changes, slot_2, &mut batch);
        exec_de
            .db
            .write()
            .write_batch(batch.clone(), DBBatch::new(), Some(slot_2));

        assert_eq!(exec_de.sorted_denunciations.len(), 1);
        assert_eq!(
            exec_de.sorted_denunciations.get(&slot_2),
            Some(&HashSet::from([de_idx_2]))
        );
        assert!(!exec_de.contains(&de_idx_1));
        assert!(exec_de.contains(&de_idx_2));

        let sorted_deunciations_1 = exec_de.sorted_denunciations.clone();

        drop(exec_de);

        // Init an exec de from disk
        let db2 = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
        ));

        let mut exec_de2 = ExecutedDenunciations::new(config, db2);
        exec_de2.recompute_sorted_denunciations();

        assert_eq!(exec_de2.sorted_denunciations, sorted_deunciations_1);

        // Reset cache
        exec_de2.reset();
        assert_eq!(exec_de2.sorted_denunciations.len(), 0);
    }
}
