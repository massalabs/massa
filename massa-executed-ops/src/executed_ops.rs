//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed operations.
//! Used to detect operation reuse.

use crate::{ops_changes::ExecutedOpsChanges, ExecutedOpsConfig};
use massa_db_exports::{
    DBBatch, MassaDBController, CF_ERROR, CRUD_ERROR, EXECUTED_OPS_ID_DESER_ERROR,
    EXECUTED_OPS_ID_SER_ERROR, EXECUTED_OPS_PREFIX, STATE_CF,
};
use massa_db_worker::MassaDB;
use massa_models::{
    operation::{OperationId, OperationIdDeserializer, OperationIdSerializer},
    prehash::PreHashSet,
    slot::{Slot, SlotDeserializer, SlotSerializer},
};
use massa_serialization::{
    BoolDeserializer, BoolSerializer, DeserializeError, Deserializer, SerializeError, Serializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use parking_lot::RwLock;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Bound::{Excluded, Included},
    sync::Arc,
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
    _config: ExecutedOpsConfig,
    /// RocksDB Instance
    pub db: Arc<RwLock<MassaDB>>,
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
    pub fn new(config: ExecutedOpsConfig, db: Arc<RwLock<MassaDB>>) -> Self {
        let slot_deserializer = SlotDeserializer::new(
            (Included(u64::MIN), Included(u64::MAX)),
            (Included(0), Excluded(config.thread_count)),
        );
        Self {
            _config: config,
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

    /// Recomputes the local caches after bootstrap or loading the state from disk
    pub fn recompute_sorted_ops_and_op_exec_status(&mut self) {
        self.sorted_ops.clear();
        self.op_exec_status.clear();

        let db = self.db.read();
        let handle = db.db.cf_handle(STATE_CF).expect(CF_ERROR);

        for (serialized_op_id, serialized_value) in db
            .db
            .prefix_iterator_cf(handle, EXECUTED_OPS_PREFIX)
            .flatten()
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
        let handle = db.db.cf_handle(STATE_CF).expect(CF_ERROR);

        let mut serialized_op_id = Vec::new();
        self.operation_id_serializer
            .serialize(op_id, &mut serialized_op_id)
            .expect(EXECUTED_OPS_ID_SER_ERROR);

        db.db
            .get_cf(handle, op_id_key!(serialized_op_id))
            .expect(CRUD_ERROR)
            .is_some()
    }

    /// Prune all operations that expire strictly before `slot`
    fn prune_to_batch(&mut self, slot: Slot, batch: &mut DBBatch) {
        let kept = self.sorted_ops.split_off(&slot);
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

        let Ok((rest, _id)) = self.operation_id_deserializer.deserialize::<DeserializeError>(&serialized_key[EXECUTED_OPS_PREFIX.len()..]) else {
            return false;
        };
        if !rest.is_empty() {
            return false;
        }

        let Ok((rest, _bool)) = self.bool_deserializer.deserialize::<DeserializeError>(serialized_value) else {
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

#[test]
fn test_executed_ops_hash_computing() {
    use massa_db_exports::STATE_HASH_INITIAL_BYTES;
    use massa_db_worker::{MassaDB, MassaDBConfig};
    use massa_hash::Hash;
    use massa_models::prehash::PreHashMap;
    use massa_models::secure_share::Id;
    use tempfile::TempDir;

    // initialize the executed ops config
    let thread_count = 2;
    let config = ExecutedOpsConfig { thread_count };
    let tempdir_a = TempDir::new().expect("cannot create temp directory");
    let tempdir_c = TempDir::new().expect("cannot create temp directory");
    let db_a_config = MassaDBConfig {
        path: tempdir_a.path().to_path_buf(),
        max_history_length: 10,
        max_new_elements: 100,
        thread_count,
    };
    let db_c_config = MassaDBConfig {
        path: tempdir_c.path().to_path_buf(),
        max_history_length: 10,
        max_new_elements: 100,
        thread_count,
    };
    let db_a = Arc::new(RwLock::new(MassaDB::new(db_a_config)));
    let db_c = Arc::new(RwLock::new(MassaDB::new(db_c_config)));
    // initialize the executed ops and executed ops changes
    let mut a = ExecutedOps::new(config.clone(), db_a.clone());
    let mut c = ExecutedOps::new(config, db_c.clone());
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
        db_a.read().get_db_hash(),
        Hash::from_bytes(STATE_HASH_INITIAL_BYTES)
    );
    assert_eq!(
        db_a.read().get_db_hash(),
        db_c.read().get_db_hash(),
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
        db_a.read().get_db_hash(),
        Hash::from_bytes(STATE_HASH_INITIAL_BYTES),
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
