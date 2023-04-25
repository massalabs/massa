//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a finite size final pool of asynchronous messages for use in the context of autonomous smart contracts

use crate::{
    changes::AsyncPoolChanges,
    config::AsyncPoolConfig,
    message::{AsyncMessage, AsyncMessageId, AsyncMessageInfo, AsyncMessageUpdate},
    AsyncMessageDeserializer, AsyncMessageIdDeserializer, AsyncMessageIdSerializer,
    AsyncMessageSerializer,
};
use massa_db::{
    write_batch, DBBatch, ASYNC_POOL_CF, ASYNC_POOL_HASH_ERROR, ASYNC_POOL_HASH_INITIAL_BYTES,
    ASYNC_POOL_HASH_KEY, CF_ERROR, CRUD_ERROR, MESSAGE_DESER_ERROR, MESSAGE_ID_DESER_ERROR,
    MESSAGE_ID_SER_ERROR, MESSAGE_SER_ERROR, METADATA_CF, WRONG_BATCH_TYPE_ERROR,
};
use massa_hash::Hash;
use massa_ledger_exports::{Applicable, SetUpdateOrDelete};
use massa_models::streaming_step::StreamingStep;
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use parking_lot::RwLock;
use rocksdb::{Direction, IteratorMode, Options, ReadOptions, DB};
use std::ops::Bound::Included;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone)]
/// Represents a pool of sorted messages in a deterministic way.
/// The final asynchronous pool is attached to the output of the latest final slot within the context of massa-final-state.
/// Nodes must bootstrap the final message pool when they join the network.
pub struct AsyncPool {
    /// Asynchronous pool configuration
    pub config: AsyncPoolConfig,
    pub db: Arc<RwLock<DB>>,
    pub message_info_cache: BTreeMap<AsyncMessageId, AsyncMessageInfo>,
    message_id_serializer: AsyncMessageIdSerializer,
    message_serializer: AsyncMessageSerializer,
    message_id_deserializer: AsyncMessageIdDeserializer,
    message_deserializer_db: AsyncMessageDeserializer,
}

impl AsyncPool {
    /// Creates an empty `AsyncPool`
    pub fn new(config: AsyncPoolConfig, db: Arc<RwLock<DB>>) -> AsyncPool {
        let mut async_pool = AsyncPool {
            config: config.clone(),
            db,
            message_info_cache: Default::default(),
            message_id_serializer: AsyncMessageIdSerializer::new(),
            message_serializer: AsyncMessageSerializer::new(true),
            message_id_deserializer: AsyncMessageIdDeserializer::new(config.thread_count),
            message_deserializer_db: AsyncMessageDeserializer::new(
                config.thread_count,
                config.max_async_message_data,
                config.max_key_length,
                true,
            ),
        };
        async_pool.recompute_message_info_cache();
        async_pool
    }

    pub fn recompute_message_info_cache(&mut self) {
        self.message_info_cache.clear();

        let db = self.db.read();
        let handle = db.cf_handle(ASYNC_POOL_CF).expect(CF_ERROR);

        for (serialized_message_id, serialized_message) in
            db.iterator_cf(handle, IteratorMode::Start).flatten()
        {
            let (_, message_id) = self
                .message_id_deserializer
                .deserialize::<DeserializeError>(&serialized_message_id)
                .expect(MESSAGE_ID_DESER_ERROR);
            let (_, message) = self
                .message_deserializer_db
                .deserialize::<DeserializeError>(&serialized_message)
                .expect(MESSAGE_DESER_ERROR);

            self.message_info_cache.insert(message_id, message.into());
        }
    }

    /// Resets the pool to its initial state
    ///
    /// USED ONLY FOR BOOTSTRAP
    pub fn reset(&mut self) {
        {
            let mut db = self.db.write();
            (*db)
                .drop_cf(ASYNC_POOL_CF)
                .expect("Error dropping async_pool cf");
            let mut db_opts = Options::default();
            db_opts.set_error_if_exists(true);
            (*db)
                .create_cf(ASYNC_POOL_CF, &db_opts)
                .expect("Error creating async_pool cf");
        }
        self.recompute_message_info_cache();
    }

    /// Applies pre-compiled `AsyncPoolChanges` to the pool without checking for overflows.
    /// This function is used when applying pre-compiled `AsyncPoolChanges` to an `AsyncPool`.
    ///
    /// # arguments
    /// * `changes`: `AsyncPoolChanges` listing all asynchronous pool changes (message insertions/deletions)
    pub fn apply_changes_to_batch(&mut self, changes: &AsyncPoolChanges, batch: &mut DBBatch) {
        for change in changes.0.iter() {
            match change {
                (id, SetUpdateOrDelete::Set(message)) => {
                    self.put_entry(id, message.clone(), batch);
                    self.message_info_cache
                        .insert(*id, AsyncMessageInfo::from(message.clone()));
                }

                (id, SetUpdateOrDelete::Update(message_update)) => {
                    self.update_entry(id, message_update.clone(), batch);

                    self.message_info_cache
                        .entry(*id)
                        .and_modify(|message_info| {
                            message_info.apply(message_update.clone());
                        });
                }

                (id, SetUpdateOrDelete::Delete) => {
                    self.delete_entry(id, batch);
                    self.message_info_cache.remove(id);
                }
            }
        }
    }

    pub fn fetch_messages<'a>(
        &self,
        message_ids: Vec<&'a AsyncMessageId>,
    ) -> Vec<(&'a AsyncMessageId, Option<AsyncMessage>)> {
        let db = self.db.read();
        let handle = db.cf_handle(ASYNC_POOL_CF).expect(CF_ERROR);

        let mut serialized_message_ids = Vec::new();

        for message_id in message_ids.iter() {
            let mut serialized_message_id = Vec::new();
            self.message_id_serializer
                .serialize(message_id, &mut serialized_message_id)
                .expect(MESSAGE_ID_SER_ERROR);
            serialized_message_ids.push((handle, serialized_message_id));
        }

        let result_messages = db.multi_get_cf(serialized_message_ids);

        let ret = message_ids
            .iter()
            .zip(result_messages)
            .map(|(message_id, serialized_message)| {
                if let Ok(Some(serialized_message)) = serialized_message {
                    let (_, message) = self
                        .message_deserializer_db
                        .deserialize::<DeserializeError>(&serialized_message)
                        .expect(MESSAGE_DESER_ERROR);
                    (*message_id, Some(message))
                } else {
                    (*message_id, None)
                }
            })
            .collect();
        ret
    }

    /// Get a part of the async pool.
    /// Used for bootstrap.
    ///
    /// # Arguments
    /// * cursor: current bootstrap state
    ///
    /// # Returns
    /// The async pool part and the updated cursor
    pub fn get_pool_part(
        &self,
        cursor: StreamingStep<AsyncMessageId>,
    ) -> (
        BTreeMap<AsyncMessageId, AsyncMessage>,
        StreamingStep<AsyncMessageId>,
    ) {
        let db = self.db.read();
        let handle = db.cf_handle(ASYNC_POOL_CF).expect(CF_ERROR);
        let opt = ReadOptions::default();

        let mut pool_part = BTreeMap::new();
        // Creates an iterator from the next element after the last if defined, otherwise initialize it at the first key of the ledger.
        let (db_iterator, mut new_cursor) = match cursor {
            StreamingStep::Started => (
                db.iterator_cf_opt(handle, opt, IteratorMode::Start),
                StreamingStep::<AsyncMessageId>::Started,
            ),
            StreamingStep::Ongoing(last_id) => {
                let mut serialized_message_id = Vec::new();
                self.message_id_serializer
                    .serialize(&last_id, &mut serialized_message_id)
                    .expect(MESSAGE_ID_SER_ERROR);
                let mut iter = db.iterator_cf_opt(
                    handle,
                    opt,
                    IteratorMode::From(&serialized_message_id, Direction::Forward),
                );
                iter.next();
                (iter, StreamingStep::Finished(None))
            }
            StreamingStep::<AsyncMessageId>::Finished(_) => return (pool_part, cursor),
        };

        // Iterates over the whole database
        for (serialized_message_id, serialized_message) in db_iterator.flatten() {
            if pool_part.len() < self.config.bootstrap_part_size as usize {
                let (_, message_id) = self
                    .message_id_deserializer
                    .deserialize::<DeserializeError>(&serialized_message_id)
                    .expect("MESSAGE_ID_DESER_ERROR");
                let (_, message) = self
                    .message_deserializer_db
                    .deserialize::<DeserializeError>(&serialized_message)
                    .expect(MESSAGE_DESER_ERROR);
                pool_part.insert(message_id, message.clone());

                new_cursor = StreamingStep::Ongoing(message_id);
            } else {
                break;
            }
        }
        (pool_part, new_cursor)
    }

    /// Set a part of the async pool.
    /// Used for bootstrap.
    ///
    /// # Arguments
    /// * part: the async pool part provided by `get_pool_part`
    ///
    /// # Returns
    /// The updated cursor after the current insert
    pub fn set_pool_part(
        &mut self,
        part: BTreeMap<AsyncMessageId, AsyncMessage>,
    ) -> StreamingStep<AsyncMessageId> {
        let mut batch = DBBatch::new(None, Some(self.get_hash()), None, None, None, None);

        let cursor = if let Some(message_id) = part.last_key_value().map(|(&id, _)| id) {
            StreamingStep::Ongoing(message_id)
        } else {
            StreamingStep::Finished(None)
        };

        for (message_id, message) in part {
            self.put_entry(&message_id, message, &mut batch);
        }

        write_batch(&self.db.read(), batch);

        cursor
    }
}
/// Serializer for `AsyncPool`
pub struct AsyncPoolSerializer {
    u64_serializer: U64VarIntSerializer,
    async_message_id_serializer: AsyncMessageIdSerializer,
    async_message_serializer: AsyncMessageSerializer,
}

impl Default for AsyncPoolSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncPoolSerializer {
    /// Creates a new `AsyncPool` serializer
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            async_message_id_serializer: AsyncMessageIdSerializer::new(),
            async_message_serializer: AsyncMessageSerializer::new(true),
        }
    }
}

impl Serializer<BTreeMap<AsyncMessageId, AsyncMessage>> for AsyncPoolSerializer {
    fn serialize(
        &self,
        value: &BTreeMap<AsyncMessageId, AsyncMessage>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        // async pool length
        self.u64_serializer
            .serialize(&(value.len() as u64), buffer)?;
        // async pool
        for (message_id, message) in value {
            self.async_message_id_serializer
                .serialize(message_id, buffer)?;
            self.async_message_serializer.serialize(message, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `AsyncPool`
pub struct AsyncPoolDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    async_message_id_deserializer: AsyncMessageIdDeserializer,
    async_message_deserializer_db: AsyncMessageDeserializer,
}

impl AsyncPoolDeserializer {
    /// Creates a new `AsyncPool` deserializer
    pub fn new(
        thread_count: u8,
        max_async_pool_length: u64,
        max_async_message_data: u64,
        max_key_length: u32,
    ) -> AsyncPoolDeserializer {
        AsyncPoolDeserializer {
            u64_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(max_async_pool_length),
            ),
            async_message_id_deserializer: AsyncMessageIdDeserializer::new(thread_count),
            async_message_deserializer_db: AsyncMessageDeserializer::new(
                thread_count,
                max_async_message_data,
                max_key_length,
                true,
            ),
        }
    }
}

impl Deserializer<BTreeMap<AsyncMessageId, AsyncMessage>> for AsyncPoolDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BTreeMap<AsyncMessageId, AsyncMessage>, E> {
        context(
            "Failed async_pool_part deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                tuple((
                    context("Failed async_message_id deserialization", |input| {
                        self.async_message_id_deserializer.deserialize(input)
                    }),
                    context("Failed async_message deserialization", |input| {
                        self.async_message_deserializer_db.deserialize(input)
                    }),
                )),
            ),
        )
        .map(|vec| vec.into_iter().collect())
        .parse(buffer)
    }
}

// Private helpers
impl AsyncPool {
    /// Add every sub-entry individually for a given entry.
    ///
    /// # Arguments
    /// * `message_id`
    /// * `message`
    /// * `batch`: the given operation batch to update
    fn put_entry(&self, message_id: &AsyncMessageId, message: AsyncMessage, batch: &mut DBBatch) {
        let db = self.db.read();
        let handle = db.cf_handle(ASYNC_POOL_CF).expect(CF_ERROR);
        let mut serialized_message_id = Vec::new();
        self.message_id_serializer
            .serialize(message_id, &mut serialized_message_id)
            .expect(MESSAGE_ID_SER_ERROR);
        let mut serialized_message = Vec::new();
        self.message_serializer
            .serialize(&message, &mut serialized_message)
            .expect(MESSAGE_SER_ERROR);

        let hash = Hash::compute_from(
            &[serialized_message_id.clone(), serialized_message.clone()].concat(),
        );
        *batch
            .async_pool_hash
            .as_mut()
            .expect(WRONG_BATCH_TYPE_ERROR) ^= hash;
        batch.aeh_list.insert(serialized_message_id.clone(), hash);
        batch
            .write_batch
            .put_cf(handle, serialized_message_id, serialized_message);
    }

    /// Update the ledger entry of a given address.
    ///
    /// # Arguments
    /// * `entry_update`: a descriptor of the entry updates to be applied
    /// * `batch`: the given operation batch to update
    fn update_entry(
        &self,
        message_id: &AsyncMessageId,
        message_update: AsyncMessageUpdate,
        batch: &mut DBBatch,
    ) {
        let db = self.db.read();

        let handle = db.cf_handle(ASYNC_POOL_CF).expect(CF_ERROR);
        let mut serialized_message_id = Vec::new();
        self.message_id_serializer
            .serialize(message_id, &mut serialized_message_id)
            .expect(MESSAGE_ID_SER_ERROR);

        if let Some(prev_bytes) = db.get_cf(handle, &serialized_message_id).expect(CRUD_ERROR) {
            *batch
                .async_pool_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^=
                Hash::compute_from(&[&serialized_message_id, &prev_bytes[..]].concat());

            let (_rest, mut message) = self
                .message_deserializer_db
                .deserialize::<DeserializeError>(&prev_bytes)
                .expect(MESSAGE_DESER_ERROR);

            message.apply(message_update);

            let mut serialized_message = Vec::new();
            self.message_serializer
                .serialize(&message, &mut serialized_message)
                .expect(MESSAGE_SER_ERROR);

            let hash = Hash::compute_from(
                &[serialized_message_id.clone(), serialized_message.clone()].concat(),
            );
            *batch
                .async_pool_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^= hash;
            batch.aeh_list.insert(serialized_message_id.clone(), hash);
            batch
                .write_batch
                .put_cf(handle, serialized_message_id, serialized_message.clone());
        }
    }

    /// Delete every sub-entry associated to the given address.
    ///
    /// # Arguments
    /// * batch: the given operation batch to update
    fn delete_entry(&self, message_id: &AsyncMessageId, batch: &mut DBBatch) {
        let db = self.db.read();
        let handle = db.cf_handle(ASYNC_POOL_CF).expect(CF_ERROR);
        let mut serialized_message_id = Vec::new();
        self.message_id_serializer
            .serialize(message_id, &mut serialized_message_id)
            .expect(MESSAGE_ID_SER_ERROR);

        if let Some(added_hash) = batch.aeh_list.get(&serialized_message_id) {
            *batch
                .async_pool_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^= *added_hash;
        } else if let Some(prev_bytes) = db
            .get_pinned_cf(handle, &serialized_message_id)
            .expect(CRUD_ERROR)
        {
            *batch
                .async_pool_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^=
                Hash::compute_from(&[&serialized_message_id, &prev_bytes[..]].concat());
        }
        batch.write_batch.delete_cf(handle, serialized_message_id);
    }

    /// Get the current async pool hash
    pub fn get_hash(&self) -> Hash {
        let db = self.db.read();
        let handle = db.cf_handle(METADATA_CF).expect(CF_ERROR);
        if let Some(async_pool_hash_bytes) = db
            .get_cf(handle, ASYNC_POOL_HASH_KEY)
            .expect(CRUD_ERROR)
            .as_deref()
        {
            Hash::from_bytes(
                async_pool_hash_bytes
                    .try_into()
                    .expect(ASYNC_POOL_HASH_ERROR),
            )
        } else {
            // initial async_hash value to avoid matching an option in every XOR operation
            // because of a one time case being an empty ledger
            // also note that the if you XOR a hash with itself result is LEDGER_HASH_INITIAL_BYTES
            Hash::from_bytes(ASYNC_POOL_HASH_INITIAL_BYTES)
        }
    }
}
