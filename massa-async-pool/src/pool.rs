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
use massa_ledger_exports::{Applicable, SetOrKeep, SetUpdateOrDelete};
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
use rocksdb::{ColumnFamily, Direction, IteratorMode, Options, ReadOptions, DB};
use std::ops::Bound::Included;
use std::{collections::BTreeMap, sync::Arc};

const EMISSION_SLOT_IDENT: u8 = 0u8;
const EMISSION_INDEX_IDENT: u8 = 1u8;
const SENDER_IDENT: u8 = 2u8;
const DESTINATION_IDENT: u8 = 3u8;
const HANDLER_IDENT: u8 = 4u8;
const MAX_GAS_IDENT: u8 = 5u8;
const FEE_IDENT: u8 = 6u8;
const COINS_IDENT: u8 = 7u8;
const VALIDITY_START_IDENT: u8 = 8u8;
const VALIDITY_END_IDENT: u8 = 9u8;
const DATA_IDENT: u8 = 10u8;
const TRIGGER_IDENT: u8 = 11u8;
const CAN_BE_EXECUTED_IDENT: u8 = 12u8;
const TOTAL_FIELDS_COUNT: u8 = 13u8;

/// Emission slot key formatting macro
#[macro_export]
macro_rules! emission_slot_key {
    ($id:expr) => {
        [&$id[..], &[EMISSION_SLOT_IDENT]].concat()
    };
}

/// Emission index key formatting macro
#[macro_export]
macro_rules! emission_index_key {
    ($id:expr) => {
        [&$id[..], &[EMISSION_INDEX_IDENT]].concat()
    };
}

/// Sender key formatting macro
#[macro_export]
macro_rules! sender_key {
    ($id:expr) => {
        [&$id[..], &[SENDER_IDENT]].concat()
    };
}

/// Destination key formatting macro
#[macro_export]
macro_rules! destination_key {
    ($id:expr) => {
        [&$id[..], &[DESTINATION_IDENT]].concat()
    };
}

/// Handler key formatting macro
#[macro_export]
macro_rules! handler_key {
    ($id:expr) => {
        [&$id[..], &[HANDLER_IDENT]].concat()
    };
}

/// Max gas key formatting macro
#[macro_export]
macro_rules! max_gas_key {
    ($id:expr) => {
        [&$id[..], &[MAX_GAS_IDENT]].concat()
    };
}

/// Fee key formatting macro
#[macro_export]
macro_rules! fee_key {
    ($id:expr) => {
        [&$id[..], &[FEE_IDENT]].concat()
    };
}

/// Coins key formatting macro
#[macro_export]
macro_rules! coins_key {
    ($id:expr) => {
        [&$id[..], &[COINS_IDENT]].concat()
    };
}

/// Validity start key formatting macro
#[macro_export]
macro_rules! validity_start_key {
    ($id:expr) => {
        [&$id[..], &[VALIDITY_START_IDENT]].concat()
    };
}

/// Validity end key formatting macro
#[macro_export]
macro_rules! validity_end_key {
    ($id:expr) => {
        [&$id[..], &[VALIDITY_END_IDENT]].concat()
    };
}

/// Data key formatting macro
#[macro_export]
macro_rules! data_key {
    ($id:expr) => {
        [&$id[..], &[DATA_IDENT]].concat()
    };
}

/// Trigger key formatting macro
#[macro_export]
macro_rules! trigger_key {
    ($id:expr) => {
        [&$id[..], &[TRIGGER_IDENT]].concat()
    };
}

/// Can be executed key formatting macro
#[macro_export]
macro_rules! can_be_executed_key {
    ($id:expr) => {
        [&$id[..], &[CAN_BE_EXECUTED_IDENT]].concat()
    };
}

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

        // Iterates over the whole database
        let mut current_id: Option<AsyncMessageId> = None;
        let mut current_message: Vec<u8> = Vec::new();
        let mut current_count = 0u8;

        for (serialized_message_id, serialized_value) in
            db.iterator_cf(handle, IteratorMode::Start).flatten()
        {
            let (_, message_id) = self
                .message_id_deserializer
                .deserialize::<DeserializeError>(&serialized_message_id)
                .expect(MESSAGE_ID_DESER_ERROR);

            if Some(message_id) == current_id {
                current_count += 1;
                current_message.extend(serialized_value.iter());
                if current_count == TOTAL_FIELDS_COUNT {
                    let (_, message) = self
                        .message_deserializer_db
                        .deserialize::<DeserializeError>(&current_message)
                        .expect(MESSAGE_DESER_ERROR);

                    self.message_info_cache.insert(message_id, message.into());

                    current_count = 0;
                    current_message.clear();
                    current_id = None;
                }
            } else {
                // We reset the current values
                current_id = Some(message_id);
                current_count = 1;
                current_message.clear();
                current_message.extend(serialized_value.iter());
            }
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
        let mut fetched_messages = Vec::new();

        for message_id in message_ids.iter() {
            let mut serialized_message_id = Vec::new();
            self.message_id_serializer
                .serialize(message_id, &mut serialized_message_id)
                .expect(MESSAGE_ID_SER_ERROR);
            serialized_message_ids.push((handle, emission_slot_key!(serialized_message_id)));
            serialized_message_ids.push((handle, emission_index_key!(serialized_message_id)));
            serialized_message_ids.push((handle, sender_key!(serialized_message_id)));
            serialized_message_ids.push((handle, destination_key!(serialized_message_id)));
            serialized_message_ids.push((handle, handler_key!(serialized_message_id)));
            serialized_message_ids.push((handle, max_gas_key!(serialized_message_id)));
            serialized_message_ids.push((handle, fee_key!(serialized_message_id)));
            serialized_message_ids.push((handle, coins_key!(serialized_message_id)));
            serialized_message_ids.push((handle, validity_start_key!(serialized_message_id)));
            serialized_message_ids.push((handle, validity_end_key!(serialized_message_id)));
            serialized_message_ids.push((handle, data_key!(serialized_message_id)));
            serialized_message_ids.push((handle, trigger_key!(serialized_message_id)));
            serialized_message_ids.push((handle, can_be_executed_key!(serialized_message_id)));
        }

        let result_messages = db.multi_get_cf(serialized_message_ids);

        'msgs_iter: for message_id in message_ids.iter() {
            let mut serialized_message: Vec<u8> = Vec::new();
            let vals = result_messages.iter().take(13);
            for val in vals {
                if let Ok(Some(serialized_value)) = val {
                    serialized_message.extend(serialized_value);
                } else {
                    fetched_messages.push((*message_id, None));
                    continue 'msgs_iter;
                }
            }
            let (_, message) = self
                .message_deserializer_db
                .deserialize::<DeserializeError>(&serialized_message)
                .expect(MESSAGE_DESER_ERROR);
            fetched_messages.push((*message_id, Some(message)));
        }

        fetched_messages
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
        let mut current_id: Option<AsyncMessageId> = None;
        let mut current_message: Vec<u8> = Vec::new();
        let mut current_count = 0u8;

        for (serialized_message_id, serialized_value) in db_iterator.flatten() {
            if pool_part.len() < self.config.bootstrap_part_size as usize {
                let (_, message_id) = self
                    .message_id_deserializer
                    .deserialize::<DeserializeError>(&serialized_message_id)
                    .expect(MESSAGE_ID_DESER_ERROR);

                if Some(message_id) == current_id {
                    current_count += 1;
                    current_message.extend(serialized_value.iter());
                    if current_count == TOTAL_FIELDS_COUNT {
                        let (_, message) = self
                            .message_deserializer_db
                            .deserialize::<DeserializeError>(&current_message)
                            .expect(MESSAGE_DESER_ERROR);

                        pool_part.insert(message_id, message.clone());
                        new_cursor = StreamingStep::Ongoing(message_id);

                        current_count = 0;
                        current_message.clear();
                        current_id = None;
                    }
                } else {
                    // We reset the current values
                    current_id = Some(message_id);
                    current_count = 1;
                    current_message.clear();
                    current_message.extend(serialized_value.iter());
                }
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
    /// Internal function to put a key & value and perform the hash XORs
    fn put_entry_value(
        &self,
        handle: &ColumnFamily,
        batch: &mut DBBatch,
        key: Vec<u8>,
        value: &[u8],
    ) {
        let hash = Hash::compute_from(&[&key, value].concat());
        *batch
            .async_pool_hash
            .as_mut()
            .expect(WRONG_BATCH_TYPE_ERROR) ^= hash;
        batch.aeh_list.insert(key.clone(), hash);
        batch.write_batch.put_cf(handle, &key, value);
    }

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

        // Emission slot
        let mut serialized_emission_slot = Vec::new();
        self.message_serializer
            .slot_serializer
            .serialize(&message.emission_slot, &mut serialized_emission_slot)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            emission_slot_key!(serialized_message_id),
            &serialized_emission_slot,
        );

        // Emission index
        let mut serialized_emission_index = Vec::new();
        self.message_serializer
            .u64_serializer
            .serialize(&message.emission_index, &mut serialized_emission_index)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            emission_index_key!(serialized_message_id),
            &serialized_emission_index,
        );

        // Sender
        let mut serialized_sender = Vec::new();
        self.message_serializer
            .address_serializer
            .serialize(&message.sender, &mut serialized_sender)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            sender_key!(serialized_message_id),
            &serialized_sender,
        );

        // Destination
        let mut serialized_destination = Vec::new();
        self.message_serializer
            .address_serializer
            .serialize(&message.destination, &mut serialized_destination)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            destination_key!(serialized_message_id),
            &serialized_destination,
        );

        // Handler
        let mut serialized_handler = Vec::new();
        let handler_bytes = message.handler.as_bytes();
        let handler_name_len: u8 = handler_bytes.len().try_into().expect(MESSAGE_SER_ERROR);
        serialized_handler.extend([handler_name_len]);
        serialized_handler.extend(handler_bytes);
        self.put_entry_value(
            handle,
            batch,
            handler_key!(serialized_message_id),
            &serialized_handler,
        );

        // Max gas
        let mut serialized_max_gas = Vec::new();
        self.message_serializer
            .u64_serializer
            .serialize(&message.max_gas, &mut serialized_max_gas)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            max_gas_key!(serialized_message_id),
            &serialized_max_gas,
        );

        // Fee
        let mut serialized_fee = Vec::new();
        self.message_serializer
            .amount_serializer
            .serialize(&message.fee, &mut serialized_fee)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            fee_key!(serialized_message_id),
            &serialized_fee,
        );

        // Coins
        let mut serialized_coins = Vec::new();
        self.message_serializer
            .amount_serializer
            .serialize(&message.coins, &mut serialized_coins)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            coins_key!(serialized_message_id),
            &serialized_coins,
        );

        // Validity start
        let mut serialized_validity_start = Vec::new();
        self.message_serializer
            .slot_serializer
            .serialize(&message.validity_start, &mut serialized_validity_start)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            validity_start_key!(serialized_message_id),
            &serialized_validity_start,
        );

        // Validity end
        let mut serialized_validity_end = Vec::new();
        self.message_serializer
            .slot_serializer
            .serialize(&message.validity_end, &mut serialized_validity_end)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            validity_end_key!(serialized_message_id),
            &serialized_validity_end,
        );

        // Data
        let mut serialized_data = Vec::new();
        self.message_serializer
            .vec_u8_serializer
            .serialize(&message.data, &mut serialized_data)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            data_key!(serialized_message_id),
            &serialized_data,
        );

        // Trigger
        let mut serialized_trigger = Vec::new();
        self.message_serializer
            .trigger_serializer
            .serialize(&message.trigger, &mut serialized_trigger)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            trigger_key!(serialized_message_id),
            &serialized_trigger,
        );

        // Can be executed
        let mut serialized_can_be_executed = Vec::new();
        self.message_serializer
            .bool_serializer
            .serialize(&message.can_be_executed, &mut serialized_can_be_executed)
            .expect(MESSAGE_SER_ERROR);
        self.put_entry_value(
            handle,
            batch,
            can_be_executed_key!(serialized_message_id),
            &serialized_can_be_executed,
        );
    }

    /// Internal function to update a key & value and perform the hash XORs
    fn update_key_value(
        &self,
        handle: &ColumnFamily,
        batch: &mut DBBatch,
        key: Vec<u8>,
        value: &[u8],
    ) {
        let db = self.db.read();
        if let Some(added_hash) = batch.aeh_list.get(&key) {
            *batch
                .async_pool_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^= *added_hash;
        } else if let Some(prev_bytes) = db.get_pinned_cf(handle, &key).expect(CRUD_ERROR) {
            *batch
                .async_pool_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^=
                Hash::compute_from(&[&key, &prev_bytes[..]].concat());
        }
        let hash = Hash::compute_from(&[&key, value].concat());
        *batch
            .async_pool_hash
            .as_mut()
            .expect(WRONG_BATCH_TYPE_ERROR) ^= hash;
        batch.aeh_list.insert(key.clone(), hash);
        batch.write_batch.put_cf(handle, key, value);
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

        // Emission slot
        if let SetOrKeep::Set(emission_slot) = message_update.emission_slot {
            let mut serialized_emission_slot = Vec::new();
            self.message_serializer
                .slot_serializer
                .serialize(&emission_slot, &mut serialized_emission_slot)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                emission_slot_key!(serialized_message_id),
                &serialized_emission_slot,
            );
        }

        // Emission index
        if let SetOrKeep::Set(emission_index) = message_update.emission_index {
            let mut serialized_emission_index = Vec::new();
            self.message_serializer
                .u64_serializer
                .serialize(&emission_index, &mut serialized_emission_index)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                emission_index_key!(serialized_message_id),
                &serialized_emission_index,
            );
        }

        // Sender
        if let SetOrKeep::Set(sender) = message_update.sender {
            let mut serialized_sender = Vec::new();
            self.message_serializer
                .address_serializer
                .serialize(&sender, &mut serialized_sender)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                sender_key!(serialized_message_id),
                &serialized_sender,
            );
        }

        // Destination
        if let SetOrKeep::Set(destination) = message_update.destination {
            let mut serialized_destination = Vec::new();
            self.message_serializer
                .address_serializer
                .serialize(&destination, &mut serialized_destination)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                destination_key!(serialized_message_id),
                &serialized_destination,
            );
        }

        // Handler
        if let SetOrKeep::Set(handler) = message_update.handler {
            let mut serialized_handler = Vec::new();
            let handler_bytes = handler.as_bytes();
            let handler_name_len: u8 = handler_bytes.len().try_into().expect(MESSAGE_SER_ERROR);
            serialized_handler.extend([handler_name_len]);
            serialized_handler.extend(handler_bytes);
            self.update_key_value(
                handle,
                batch,
                handler_key!(serialized_message_id),
                &serialized_handler,
            );
        }

        // Max gas
        if let SetOrKeep::Set(max_gas) = message_update.max_gas {
            let mut serialized_max_gas = Vec::new();
            self.message_serializer
                .u64_serializer
                .serialize(&max_gas, &mut serialized_max_gas)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                max_gas_key!(serialized_message_id),
                &serialized_max_gas,
            );
        }

        // Fee
        if let SetOrKeep::Set(fee) = message_update.fee {
            let mut serialized_fee = Vec::new();
            self.message_serializer
                .amount_serializer
                .serialize(&fee, &mut serialized_fee)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                fee_key!(serialized_message_id),
                &serialized_fee,
            );
        }

        // Coins
        if let SetOrKeep::Set(coins) = message_update.coins {
            let mut serialized_coins = Vec::new();
            self.message_serializer
                .amount_serializer
                .serialize(&coins, &mut serialized_coins)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                coins_key!(serialized_message_id),
                &serialized_coins,
            );
        }

        // Validity start
        if let SetOrKeep::Set(validity_start) = message_update.validity_start {
            let mut serialized_validity_start = Vec::new();
            self.message_serializer
                .slot_serializer
                .serialize(&validity_start, &mut serialized_validity_start)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                validity_start_key!(serialized_message_id),
                &serialized_validity_start,
            );
        }

        // Validity end
        if let SetOrKeep::Set(validity_end) = message_update.validity_end {
            let mut serialized_validity_end = Vec::new();
            self.message_serializer
                .slot_serializer
                .serialize(&validity_end, &mut serialized_validity_end)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                validity_end_key!(serialized_message_id),
                &serialized_validity_end,
            );
        }

        // Data
        if let SetOrKeep::Set(data) = message_update.data {
            let mut serialized_data = Vec::new();
            self.message_serializer
                .vec_u8_serializer
                .serialize(&data, &mut serialized_data)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                data_key!(serialized_message_id),
                &serialized_data,
            );
        }

        // Trigger
        if let SetOrKeep::Set(trigger) = message_update.trigger {
            let mut serialized_trigger = Vec::new();
            self.message_serializer
                .trigger_serializer
                .serialize(&trigger, &mut serialized_trigger)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                trigger_key!(serialized_message_id),
                &serialized_trigger,
            );
        }

        // Can be executed
        if let SetOrKeep::Set(can_be_executed) = message_update.can_be_executed {
            let mut serialized_can_be_executed = Vec::new();
            self.message_serializer
                .bool_serializer
                .serialize(&can_be_executed, &mut serialized_can_be_executed)
                .expect(MESSAGE_SER_ERROR);
            self.update_key_value(
                handle,
                batch,
                can_be_executed_key!(serialized_message_id),
                &serialized_can_be_executed,
            );
        }
    }

    /// Internal function to delete a key and perform the hash XOR
    fn delete_key(&self, handle: &ColumnFamily, batch: &mut DBBatch, key: Vec<u8>) {
        let db = self.db.read();
        if let Some(added_hash) = batch.aeh_list.get(&key) {
            *batch
                .async_pool_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^= *added_hash;
        } else if let Some(prev_bytes) = db.get_pinned_cf(handle, &key).expect(CRUD_ERROR) {
            *batch
                .async_pool_hash
                .as_mut()
                .expect(WRONG_BATCH_TYPE_ERROR) ^=
                Hash::compute_from(&[&key, &prev_bytes[..]].concat());
        }
        batch.write_batch.delete_cf(handle, key);
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

        self.delete_key(handle, batch, emission_slot_key!(serialized_message_id));
        self.delete_key(handle, batch, emission_index_key!(serialized_message_id));
        self.delete_key(handle, batch, sender_key!(serialized_message_id));
        self.delete_key(handle, batch, destination_key!(serialized_message_id));
        self.delete_key(handle, batch, handler_key!(serialized_message_id));
        self.delete_key(handle, batch, max_gas_key!(serialized_message_id));
        self.delete_key(handle, batch, fee_key!(serialized_message_id));
        self.delete_key(handle, batch, coins_key!(serialized_message_id));
        self.delete_key(handle, batch, validity_start_key!(serialized_message_id));
        self.delete_key(handle, batch, validity_end_key!(serialized_message_id));
        self.delete_key(handle, batch, data_key!(serialized_message_id));
        self.delete_key(handle, batch, trigger_key!(serialized_message_id));
        self.delete_key(handle, batch, can_be_executed_key!(serialized_message_id));
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
