//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a structure to list and prune previously executed operations.
//! Used to detect operation reuse.

use crate::{ops_changes::ExecutedOpsChanges, ExecutedOpsConfig};
use massa_db::{
    write_batch, DBBatch, CF_ERROR, CRUD_ERROR, EXECUTED_OPS_CF, EXECUTED_OPS_HASH_ERROR,
    EXECUTED_OPS_HASH_INITIAL_BYTES, EXECUTED_OPS_HASH_KEY, EXECUTED_OPS_ID_DESER_ERROR,
    EXECUTED_OPS_ID_SER_ERROR, METADATA_CF, WRONG_BATCH_TYPE_ERROR,
};
use massa_hash::Hash;
use massa_models::{
    operation::{OperationId, OperationIdDeserializer, OperationIdSerializer},
    prehash::PreHashSet,
    secure_share::Id,
    slot::{Slot, SlotDeserializer, SlotSerializer},
    streaming_step::StreamingStep,
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
use rocksdb::{IteratorMode, Options, DB};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Bound::{Excluded, Included, Unbounded},
    sync::Arc,
};

/// A structure to list and prune previously executed operations
#[derive(Clone)]
pub struct ExecutedOps {
    /// Executed operations configuration
    config: ExecutedOpsConfig,
    /// RocksDB Instance
    pub db: Arc<RwLock<DB>>,
    /// Executed operations btreemap with slot as index for better pruning complexity
    pub sorted_ops: BTreeMap<Slot, PreHashSet<OperationId>>,
    /// execution status of operations (true: success, false: fail)
    pub op_exec_status: HashMap<OperationId, bool>,
    operation_id_deserializer: OperationIdDeserializer,
    operation_id_serializer: OperationIdSerializer,
    bool_serializer: BoolSerializer,
    slot_serializer: SlotSerializer,
}

impl ExecutedOps {
    /// Creates a new `ExecutedOps`
    pub fn new(config: ExecutedOpsConfig, db: Arc<RwLock<DB>>) -> Self {
        Self {
            config,
            db,
            sorted_ops: BTreeMap::new(),
            op_exec_status: HashMap::new(),
            operation_id_deserializer: OperationIdDeserializer::new(),
            operation_id_serializer: OperationIdSerializer::new(),
            bool_serializer: BoolSerializer::new(),
            slot_serializer: SlotSerializer::new(),
        }
    }

    fn recompute_sorted_ops_and_op_exec_status(&mut self) {
        self.sorted_ops.clear();
        self.op_exec_status.clear();

        let db = self.db.read();
        let handle = db.cf_handle(EXECUTED_OPS_CF).expect(CF_ERROR);

        let bool_deserializer = BoolDeserializer::new();
        let slot_deserializer = SlotDeserializer::new(
            (Included(u64::MIN), Included(u64::MAX)),
            (Included(0), Excluded(self.config.thread_count)),
        );

        for (serialized_op_id, serialized_value) in
            db.iterator_cf(handle, IteratorMode::Start).flatten()
        {
            let (_, op_id) = self
                .operation_id_deserializer
                .deserialize::<DeserializeError>(&serialized_op_id)
                .expect(EXECUTED_OPS_ID_DESER_ERROR);

            let (rest, op_exec_status) = bool_deserializer
                .deserialize::<DeserializeError>(&serialized_value)
                .expect(EXECUTED_OPS_ID_DESER_ERROR);
            let (_, slot) = slot_deserializer
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
        {
            let mut db = self.db.write();
            (*db)
                .drop_cf(EXECUTED_OPS_CF)
                .expect("Error dropping executed_ops cf");
            let mut db_opts = Options::default();
            db_opts.set_error_if_exists(true);
            (*db)
                .create_cf(EXECUTED_OPS_CF, &db_opts)
                .expect("Error creating executed_ops cf");
        }

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
        let handle = db.cf_handle(EXECUTED_OPS_CF).expect(CF_ERROR);

        let mut serialized_op_id = Vec::new();
        self.operation_id_serializer
            .serialize(op_id, &mut serialized_op_id)
            .expect(EXECUTED_OPS_ID_SER_ERROR);

        db.get_cf(handle, &serialized_op_id)
            .expect(CRUD_ERROR)
            .is_some()
    }

    /// Prune all operations that expire strictly before `slot`
    fn prune_to_batch(&mut self, slot: Slot, batch: &mut DBBatch) {
        let kept = self.sorted_ops.split_off(&slot);
        let removed = std::mem::take(&mut self.sorted_ops);
        for (_, ids) in removed {
            for op_id in ids {
                self.delete_entry(&op_id, batch);
            }
        }
        self.sorted_ops = kept;
    }

    /// Add a denunciation_index to the DB
    ///
    /// # Arguments
    /// * `message_id`
    /// * `message`
    /// * `batch`: the given operation batch to update
    fn put_entry(&self, op_id: &OperationId, value: &(bool, Slot), batch: &mut DBBatch) {
        let db = self.db.read();
        let handle = db.cf_handle(EXECUTED_OPS_CF).expect(CF_ERROR);

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

        if db
            .get_cf(handle, &serialized_op_id)
            .expect(CRUD_ERROR)
            .is_none()
        {
            let hash = op_id.get_hash();
            batch.aeh_list.insert(serialized_op_id.clone(), *hash);
            *batch
                .executed_ops_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^= *hash;
            batch
                .write_batch
                .put_cf(handle, serialized_op_id, serialized_op_value);
        }
    }

    /// Remove a denunciation_index from the DB
    ///
    /// # Arguments
    /// * batch: the given operation batch to update
    fn delete_entry(&self, op_id: &OperationId, batch: &mut DBBatch) {
        let db = self.db.read();
        let handle = db.cf_handle(EXECUTED_OPS_CF).expect(CF_ERROR);

        let mut serialized_op_id = Vec::new();
        self.operation_id_serializer
            .serialize(op_id, &mut serialized_op_id)
            .expect(EXECUTED_OPS_ID_SER_ERROR);

        if let Some(added_hash) = batch.aeh_list.get(&serialized_op_id) {
            *batch
                .executed_ops_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^= *added_hash;
        } else if db
            .get_pinned_cf(handle, &serialized_op_id)
            .expect(CRUD_ERROR)
            .is_some()
        {
            let hash = op_id.get_hash();
            *batch
                .executed_ops_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^= *hash
        }
        batch.write_batch.delete_cf(handle, serialized_op_id);
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
        let mut batch = DBBatch::new(None, None, None, None, Some(self.get_hash()), None);

        self.sorted_ops.extend(part.clone());

        for (slot, ids) in part.iter() {
            for op_id in ids.iter() {
                let value = (false, *slot);
                self.put_entry(op_id, &value, &mut batch);
            }
        }

        write_batch(&self.db.read(), batch);

        if let Some(slot) = self.sorted_ops.last_key_value().map(|(slot, _)| slot) {
            StreamingStep::Ongoing(*slot)
        } else {
            StreamingStep::Finished(None)
        }
    }

    /// Get the current executed ops hash
    pub fn get_hash(&self) -> Hash {
        let db = self.db.read();
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);
        if let Some(executed_denunciations_hash) = db
            .get_cf(handle, EXECUTED_OPS_HASH_KEY)
            .expect(CRUD_ERROR)
            .as_deref()
        {
            Hash::from_bytes(
                executed_denunciations_hash
                    .try_into()
                    .expect(EXECUTED_OPS_HASH_ERROR),
            )
        } else {
            // initial executed_denunciations value to avoid matching an option in every XOR operation
            // because of a one time case being an empty ledger
            // also note that the if you XOR a hash with itself result is EXECUTED_DENUNCIATIONS_HASH_INITIAL_BYTES
            Hash::from_bytes(EXECUTED_OPS_HASH_INITIAL_BYTES)
        }
    }
}

#[test]
fn test_executed_ops_xor_computing() {
    use massa_db::new_rocks_db_instance;
    use massa_db::write_batch;
    use massa_models::prehash::PreHashMap;
    use massa_models::secure_share::Id;
    use tempfile::TempDir;

    // initialize the executed ops config
    let config = ExecutedOpsConfig {
        thread_count: 2,
        bootstrap_part_size: 10,
    };
    let tempdir_a = TempDir::new().expect("cannot create temp directory");
    let tempdir_c = TempDir::new().expect("cannot create temp directory");
    let rocks_db_instance_a = Arc::new(RwLock::new(new_rocks_db_instance(
        tempdir_a.path().to_path_buf(),
    )));
    let rocks_db_instance_c = Arc::new(RwLock::new(new_rocks_db_instance(
        tempdir_c.path().to_path_buf(),
    )));
    // initialize the executed ops and executed ops changes
    let mut a = ExecutedOps::new(config.clone(), rocks_db_instance_a.clone());
    let mut c = ExecutedOps::new(config, rocks_db_instance_c.clone());
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

    let mut batch_a = DBBatch::new(None, None, None, None, Some(a.get_hash()), None);
    let mut batch_c = DBBatch::new(None, None, None, None, Some(c.get_hash()), None);
    a.apply_changes_to_batch(change_a, apply_slot, &mut batch_a);
    write_batch(&rocks_db_instance_a.read(), batch_a);
    let mut batch_a = DBBatch::new(None, None, None, None, Some(a.get_hash()), None);

    a.apply_changes_to_batch(change_b, apply_slot, &mut batch_a);
    c.apply_changes_to_batch(change_c, apply_slot, &mut batch_c);
    write_batch(&rocks_db_instance_a.read(), batch_a);
    write_batch(&rocks_db_instance_c.read(), batch_c);

    // check that a.hash ^ $(change_b) = c.hash
    assert_eq!(
        a.get_hash(),
        c.get_hash(),
        "'a' and 'c' hashes are not equal"
    );

    // prune every element
    let prune_slot = Slot {
        period: 20,
        thread: 0,
    };
    let mut batch_a = DBBatch::new(None, None, None, None, Some(a.get_hash()), None);
    a.apply_changes_to_batch(PreHashMap::default(), prune_slot, &mut batch_a);
    a.prune_to_batch(prune_slot, &mut batch_a);
    write_batch(&rocks_db_instance_a.read(), batch_a);

    // at this point the hash should have been XORed with itself
    assert_eq!(
        a.get_hash(),
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
