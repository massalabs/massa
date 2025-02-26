//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed operations.
//! Used to detect operation reuse.

use crate::{ops_changes::ExecutedOpsChanges, ExecutedOpsConfig};
use massa_db_exports::{
    DBBatch, ShareableMassaDBController, CRUD_ERROR, EXECUTED_OPS_ID_DESER_ERROR,
    EXECUTED_OPS_ID_SER_ERROR, EXECUTED_OPS_PREFIX, STATE_CF,
};
use massa_models::{
    operation::{OperationId, OperationIdDeserializer, OperationIdSerializer},
    prehash::PreHashSet,
    slot::{Slot, SlotDeserializer, SlotSerializer},
};
use massa_serialization::{
    BoolDeserializer, BoolSerializer, DeserializeError, Deserializer, Serializer,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Bound::{Excluded, Included},
};

/// Op id key formatting macro
#[macro_export]
macro_rules! op_id_key {
    ($id:expr) => {
        [&EXECUTED_OPS_PREFIX.as_bytes(), &$id[..]].concat()
    };
}

/// A structure to list and prune previously executed operations
#[derive(Clone)]
pub struct ExecutedOps {
    /// Executed operations configuration
    config: ExecutedOpsConfig,
    /// RocksDB Instance
    pub db: ShareableMassaDBController,
    /// Executed operations btreemap with slot as index for better pruning complexity
    pub sorted_ops: BTreeMap<Slot, PreHashSet<OperationId>>,
    /// execution status of operations (true: success, false: fail)
    pub op_exec_status: HashMap<OperationId, bool>,
    operation_id_deserializer: OperationIdDeserializer,
    operation_id_serializer: OperationIdSerializer,
    bool_deserializer: BoolDeserializer,
    bool_serializer: BoolSerializer,
    slot_deserializer: SlotDeserializer,
    slot_serializer: SlotSerializer,
}

impl ExecutedOps {
    /// Creates a new `ExecutedOps`
    pub fn new(config: ExecutedOpsConfig, db: ShareableMassaDBController) -> Self {
        let slot_deserializer = SlotDeserializer::new(
            (Included(u64::MIN), Included(u64::MAX)),
            (Included(0), Excluded(config.thread_count)),
        );
        Self {
            config,
            db,
            sorted_ops: BTreeMap::new(),
            op_exec_status: HashMap::new(),
            operation_id_deserializer: OperationIdDeserializer::new(),
            operation_id_serializer: OperationIdSerializer::new(),
            bool_deserializer: BoolDeserializer::new(),
            bool_serializer: BoolSerializer::new(),
            slot_deserializer,
            slot_serializer: SlotSerializer::new(),
        }
    }

    /// Get the execution statuses of a set of operations.
    /// Returns a list where each element is None if no execution was found for that op,
    /// or a boolean indicating whether the execution was successful (true) or had an error (false).
    pub fn get_ops_exec_status(&self, batch: &[OperationId]) -> Vec<Option<bool>> {
        batch
            .iter()
            .map(|op_id| self.op_exec_status.get(op_id).copied())
            .collect()
    }

    /// Recomputes the local caches after bootstrap or loading the state from disk
    pub fn recompute_sorted_ops_and_op_exec_status(&mut self) {
        self.sorted_ops.clear();
        self.op_exec_status.clear();

        let db = self.db.read();

        for (serialized_op_id, serialized_value) in
            db.prefix_iterator_cf(STATE_CF, EXECUTED_OPS_PREFIX.as_bytes())
        {
            if !serialized_op_id.starts_with(EXECUTED_OPS_PREFIX.as_bytes()) {
                break;
            }

            let (_, op_id) = self
                .operation_id_deserializer
                .deserialize::<DeserializeError>(&serialized_op_id[EXECUTED_OPS_PREFIX.len()..])
                .expect(EXECUTED_OPS_ID_DESER_ERROR);

            let (rest, op_exec_status) = self
                .bool_deserializer
                .deserialize::<DeserializeError>(&serialized_value)
                .expect(EXECUTED_OPS_ID_DESER_ERROR);
            let (_, slot) = self
                .slot_deserializer
                .deserialize::<DeserializeError>(rest)
                .expect(EXECUTED_OPS_ID_DESER_ERROR);

            self.sorted_ops
                .entry(slot)
                .and_modify(|ids| {
                    ids.insert(op_id);
                })
                .or_insert_with(|| {
                    let mut new = HashSet::default();
                    new.insert(op_id);
                    new
                });
            self.op_exec_status.insert(op_id, op_exec_status);
        }
    }

    /// Reset the executed operations
    ///
    /// USED FOR BOOTSTRAP ONLY
    pub fn reset(&mut self) {
        self.db
            .write()
            .delete_prefix(EXECUTED_OPS_PREFIX, STATE_CF, None);

        self.recompute_sorted_ops_and_op_exec_status();
    }

    /// Apply speculative operations changes to the final executed operations state
    pub fn apply_changes_to_batch(
        &mut self,
        changes: ExecutedOpsChanges,
        slot: Slot,
        batch: &mut DBBatch,
    ) {
        for (id, value) in changes.iter() {
            self.put_entry(id, value, batch);
        }

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

        self.prune_to_batch(slot, batch);
    }

    /// Check if an operation was executed
    pub fn contains(&self, op_id: &OperationId) -> bool {
        let db = self.db.read();

        let mut serialized_op_id = Vec::new();
        self.operation_id_serializer
            .serialize(op_id, &mut serialized_op_id)
            .expect(EXECUTED_OPS_ID_SER_ERROR);

        db.get_cf(STATE_CF, op_id_key!(serialized_op_id))
            .expect(CRUD_ERROR)
            .is_some()
    }

    /// Prune all expired operations
    fn prune_to_batch(&mut self, slot: Slot, batch: &mut DBBatch) {
        // Force-keep `keep_executed_history_extra_periods` for API polling safety
        let cutoff_slot = match slot
            .period
            .checked_sub(self.config.keep_executed_history_extra_periods)
        {
            Some(cutoff_slot) => Slot::new(cutoff_slot, slot.thread),
            None => return,
        };

        let kept = self.sorted_ops.split_off(&cutoff_slot);
        let removed = std::mem::take(&mut self.sorted_ops);
        for (_, ids) in removed {
            for op_id in ids {
                self.op_exec_status.remove(&op_id);
                self.delete_entry(&op_id, batch);
            }
        }
        self.sorted_ops = kept;
    }

    /// Add an executed_op to the DB
    ///
    /// # Arguments
    /// * `op_id`
    /// * `value`: execution status and validity slot
    /// * `batch`: the given operation batch to update
    fn put_entry(&self, op_id: &OperationId, value: &(bool, Slot), batch: &mut DBBatch) {
        let db = self.db.read();

        let mut serialized_op_id = Vec::new();
        self.operation_id_serializer
            .serialize(op_id, &mut serialized_op_id)
            .expect(EXECUTED_OPS_ID_SER_ERROR);

        let mut serialized_op_value = Vec::new();
        self.bool_serializer
            .serialize(&value.0, &mut serialized_op_value)
            .expect(EXECUTED_OPS_ID_SER_ERROR);
        self.slot_serializer
            .serialize(&value.1, &mut serialized_op_value)
            .expect(EXECUTED_OPS_ID_SER_ERROR);

        db.put_or_update_entry_value(batch, op_id_key!(serialized_op_id), &serialized_op_value);
    }

    /// Remove a op_id from the DB
    ///
    /// # Arguments
    /// * batch: the given operation batch to update
    fn delete_entry(&self, op_id: &OperationId, batch: &mut DBBatch) {
        let db = self.db.read();

        let mut serialized_op_id = Vec::new();
        self.operation_id_serializer
            .serialize(op_id, &mut serialized_op_id)
            .expect(EXECUTED_OPS_ID_SER_ERROR);

        db.delete_key(batch, op_id_key!(serialized_op_id));
    }

    /// Deserializes the key and value, useful after bootstrap
    pub fn is_key_value_valid(&self, serialized_key: &[u8], serialized_value: &[u8]) -> bool {
        if !serialized_key.starts_with(EXECUTED_OPS_PREFIX.as_bytes()) {
            return false;
        }

        let Ok((rest, _id)): Result<(&[u8], OperationId), nom::Err<DeserializeError>> = self
            .operation_id_deserializer
            .deserialize::<DeserializeError>(&serialized_key[EXECUTED_OPS_PREFIX.len()..])
        else {
            return false;
        };
        if !rest.is_empty() {
            return false;
        }

        let Ok((rest, _bool)) = self
            .bool_deserializer
            .deserialize::<DeserializeError>(serialized_value)
        else {
            return false;
        };
        let Ok((rest, _slot)) = self.slot_deserializer.deserialize::<DeserializeError>(rest) else {
            return false;
        };
        if !rest.is_empty() {
            return false;
        }

        true
    }
}
#[cfg(test)]
mod test {
    use std::sync::Arc;

    use parking_lot::RwLock;
    use tempfile::{tempdir, TempDir};

    use massa_db_exports::{MassaDBConfig, MassaDBController, STATE_HASH_INITIAL_BYTES};
    use massa_db_worker::MassaDB;
    use massa_hash::{Hash, HashXof};
    use massa_models::config::{KEEP_EXECUTED_HISTORY_EXTRA_PERIODS, THREAD_COUNT};
    use massa_models::prehash::PreHashMap;
    use massa_models::secure_share::Id;

    use super::*;

    #[test]
    fn test_executed_ops_cache() {
        // initialize the executed ops config
        // let thread_count = 2;
        let config = ExecutedOpsConfig {
            thread_count: THREAD_COUNT,
            keep_executed_history_extra_periods: KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
        };

        // Db init
        let temp_dir = tempdir().expect("Unable to create a temp folder");
        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
            enable_metrics: true,
        };
        let db = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config.clone())) as Box<(dyn MassaDBController + 'static)>
        ));

        let mut exec_ops = ExecutedOps::new(config.clone(), db.clone());

        let mut changes = PreHashMap::default();

        let slot_1 = Slot::new(1, 0);
        let op_id_1 = OperationId::new(Hash::compute_from(&[0]));
        changes.insert(op_id_1, (true, slot_1));
        let slot_2 = Slot::new(KEEP_EXECUTED_HISTORY_EXTRA_PERIODS + 2, 3);
        let op_id_2 = OperationId::new(Hash::compute_from(&[1]));
        changes.insert(op_id_2, (true, slot_2));

        let mut batch = DBBatch::new();
        exec_ops.apply_changes_to_batch(changes, slot_2, &mut batch);
        db.write().write_batch(batch, Default::default(), None);

        // cache len expected is 1, expect op_id_1 to be discarded (as it is tool old)
        assert_eq!(exec_ops.sorted_ops.len(), 1);
        assert!(!exec_ops.contains(&op_id_1));
        assert!(exec_ops.contains(&op_id_2));

        let sorted_ops_1 = exec_ops.sorted_ops.clone();
        drop(db);
        drop(exec_ops);

        let db2 = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
        ));

        // After an init from disk, cache is empty, so recompute it and compare with original
        let mut exec_ops2 = ExecutedOps::new(config, db2.clone());
        exec_ops2.recompute_sorted_ops_and_op_exec_status();
        assert_eq!(exec_ops2.sorted_ops, sorted_ops_1);

        // Reset cache
        exec_ops2.reset();
        assert_eq!(exec_ops2.sorted_ops.len(), 0);
    }

    #[test]
    fn test_executed_ops_hash_computing() {
        // initialize the executed ops config
        let thread_count = 2;
        let config = ExecutedOpsConfig {
            thread_count,
            keep_executed_history_extra_periods: 2,
        };
        let tempdir_a = TempDir::new().expect("cannot create temp directory");
        let tempdir_c = TempDir::new().expect("cannot create temp directory");
        let db_a_config = MassaDBConfig {
            path: tempdir_a.path().to_path_buf(),
            max_history_length: 10,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count,
            max_ledger_backups: 10,
            enable_metrics: true,
        };
        let db_c_config = MassaDBConfig {
            path: tempdir_c.path().to_path_buf(),
            max_history_length: 10,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count,
            max_ledger_backups: 10,
            enable_metrics: true,
        };

        let db_a = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_a_config)) as Box<(dyn MassaDBController + 'static)>
        ));
        let db_c = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_c_config)) as Box<(dyn MassaDBController + 'static)>
        ));

        // initialize the executed ops and executed ops changes
        let mut a = ExecutedOps::new(config.clone(), db_a.clone());
        let mut c = ExecutedOps::new(config.clone(), db_c.clone());
        let mut change_a = PreHashMap::default();
        let mut change_b = PreHashMap::default();
        let mut change_c = PreHashMap::default();
        for i in 0u8..20 {
            let expiration_slot = Slot {
                period: (i as u64).saturating_sub(config.keep_executed_history_extra_periods),
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

        let mut batch_a = DBBatch::new();
        a.apply_changes_to_batch(change_a, apply_slot, &mut batch_a);
        db_a.write().write_batch(batch_a, Default::default(), None);

        let mut batch_b = DBBatch::new();
        a.apply_changes_to_batch(change_b, apply_slot, &mut batch_b);
        db_a.write().write_batch(batch_b, Default::default(), None);

        let mut batch_c = DBBatch::new();
        c.apply_changes_to_batch(change_c, apply_slot, &mut batch_c);
        db_c.write().write_batch(batch_c, Default::default(), None);

        // check that a.hash ^ $(change_b) = c.hash
        assert_ne!(
            db_a.read().get_xof_db_hash(),
            HashXof(*STATE_HASH_INITIAL_BYTES)
        );
        assert_eq!(
            db_a.read().get_xof_db_hash(),
            db_c.read().get_xof_db_hash(),
            "'a' and 'c' hashes are not equal"
        );

        // prune every element
        let prune_slot = Slot {
            period: 20,
            thread: 0,
        };
        let mut batch_a = DBBatch::new();
        a.prune_to_batch(prune_slot, &mut batch_a);
        db_a.write().write_batch(batch_a, Default::default(), None);

        // at this point the hash should have been reset to its original value
        assert_eq!(
            db_a.read().get_xof_db_hash(),
            HashXof(*STATE_HASH_INITIAL_BYTES),
            "'a' was not reset to its initial value"
        );
    }
}
