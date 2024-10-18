//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a finite size final pool of asynchronous messages for use in the context of autonomous smart contracts

use crate::{
    changes::AsyncPoolChanges,
    config::AsyncPoolConfig,
    message::{AsyncMessage, AsyncMessageId, AsyncMessageInfo, AsyncMessageUpdate},
    AsyncMessageDeserializer, AsyncMessageIdDeserializer, AsyncMessageIdSerializer,
    AsyncMessageSerializer,
};
use massa_db_exports::{
    DBBatch, MassaDirection, MassaIteratorMode, ShareableMassaDBController, ASYNC_POOL_PREFIX,
    MESSAGE_ID_DESER_ERROR, MESSAGE_ID_SER_ERROR, MESSAGE_SER_ERROR, STATE_CF,
};
use massa_models::address::Address;
use massa_models::types::{Applicable, SetOrKeep, SetUpdateOrDelete};
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
use std::collections::BTreeMap;
use std::ops::Bound::Included;

const EMISSION_SLOT_IDENT: u8 = 0u8;
const EMISSION_INDEX_IDENT: u8 = 1u8;
const SENDER_IDENT: u8 = 2u8;
const DESTINATION_IDENT: u8 = 3u8;
const FUNCTION_IDENT: u8 = 4u8;
const MAX_GAS_IDENT: u8 = 5u8;
const FEE_IDENT: u8 = 6u8;
const COINS_IDENT: u8 = 7u8;
const VALIDITY_START_IDENT: u8 = 8u8;
const VALIDITY_END_IDENT: u8 = 9u8;
const FUNCTION_PARAMS_IDENT: u8 = 10u8;
const TRIGGER_IDENT: u8 = 11u8;
const CAN_BE_EXECUTED_IDENT: u8 = 12u8;

/// Emission slot key formatting macro
#[macro_export]
macro_rules! emission_slot_key {
    ($id:expr) => {
        [
            &ASYNC_POOL_PREFIX.as_bytes(),
            &$id[..],
            &[EMISSION_SLOT_IDENT],
        ]
        .concat()
    };
}

/// Emission index key formatting macro
#[macro_export]
macro_rules! emission_index_key {
    ($id:expr) => {
        [
            &ASYNC_POOL_PREFIX.as_bytes(),
            &$id[..],
            &[EMISSION_INDEX_IDENT],
        ]
        .concat()
    };
}

/// Sender key formatting macro
#[macro_export]
macro_rules! sender_key {
    ($id:expr) => {
        [&ASYNC_POOL_PREFIX.as_bytes(), &$id[..], &[SENDER_IDENT]].concat()
    };
}

/// Destination key formatting macro
#[macro_export]
macro_rules! destination_key {
    ($id:expr) => {
        [
            &ASYNC_POOL_PREFIX.as_bytes(),
            &$id[..],
            &[DESTINATION_IDENT],
        ]
        .concat()
    };
}

/// Function name key formatting macro
#[macro_export]
macro_rules! function_key {
    ($id:expr) => {
        [&ASYNC_POOL_PREFIX.as_bytes(), &$id[..], &[FUNCTION_IDENT]].concat()
    };
}

/// Max gas key formatting macro
#[macro_export]
macro_rules! max_gas_key {
    ($id:expr) => {
        [&ASYNC_POOL_PREFIX.as_bytes(), &$id[..], &[MAX_GAS_IDENT]].concat()
    };
}

/// Fee key formatting macro
#[macro_export]
macro_rules! fee_key {
    ($id:expr) => {
        [&ASYNC_POOL_PREFIX.as_bytes(), &$id[..], &[FEE_IDENT]].concat()
    };
}

/// Coins key formatting macro
#[macro_export]
macro_rules! coins_key {
    ($id:expr) => {
        [&ASYNC_POOL_PREFIX.as_bytes(), &$id[..], &[COINS_IDENT]].concat()
    };
}

/// Validity start key formatting macro
#[macro_export]
macro_rules! validity_start_key {
    ($id:expr) => {
        [
            &ASYNC_POOL_PREFIX.as_bytes(),
            &$id[..],
            &[VALIDITY_START_IDENT],
        ]
        .concat()
    };
}

/// Validity end key formatting macro
#[macro_export]
macro_rules! validity_end_key {
    ($id:expr) => {
        [
            &ASYNC_POOL_PREFIX.as_bytes(),
            &$id[..],
            &[VALIDITY_END_IDENT],
        ]
        .concat()
    };
}

/// Function params key formatting macro
#[macro_export]
macro_rules! function_params_key {
    ($id:expr) => {
        [
            &ASYNC_POOL_PREFIX.as_bytes(),
            &$id[..],
            &[FUNCTION_PARAMS_IDENT],
        ]
        .concat()
    };
}

/// Trigger key formatting macro
#[macro_export]
macro_rules! trigger_key {
    ($id:expr) => {
        [&ASYNC_POOL_PREFIX.as_bytes(), &$id[..], &[TRIGGER_IDENT]].concat()
    };
}

/// Can be executed key formatting macro
#[macro_export]
macro_rules! can_be_executed_key {
    ($id:expr) => {
        [
            &ASYNC_POOL_PREFIX.as_bytes(),
            &$id[..],
            &[CAN_BE_EXECUTED_IDENT],
        ]
        .concat()
    };
}

/// Message id prefix formatting macro
#[macro_export]
macro_rules! message_id_prefix {
    ($id:expr) => {
        [&ASYNC_POOL_PREFIX.as_bytes(), &$id[..]].concat()
    };
}

#[derive(Clone)]
/// Represents a pool of sorted messages in a deterministic way.
/// The final asynchronous pool is attached to the output of the latest final slot within the context of massa-final-state.
/// Nodes must bootstrap the final message pool when they join the network.
pub struct AsyncPool {
    /// Asynchronous pool configuration
    pub config: AsyncPoolConfig,
    pub db: ShareableMassaDBController,
    pub message_info_cache: BTreeMap<AsyncMessageId, AsyncMessageInfo>,
    message_id_serializer: AsyncMessageIdSerializer,
    message_serializer: AsyncMessageSerializer,
    message_id_deserializer: AsyncMessageIdDeserializer,
    message_deserializer_db: AsyncMessageDeserializer,
}

impl AsyncPool {
    /// Creates an empty `AsyncPool`
    pub fn new(config: AsyncPoolConfig, db: ShareableMassaDBController) -> AsyncPool {
        AsyncPool {
            config: config.clone(),
            db,
            message_info_cache: Default::default(),
            message_id_serializer: AsyncMessageIdSerializer::new(),
            message_serializer: AsyncMessageSerializer::new(true),
            message_id_deserializer: AsyncMessageIdDeserializer::new(config.thread_count),
            message_deserializer_db: AsyncMessageDeserializer::new(
                config.thread_count,
                config.max_function_length,
                config.max_function_params_length,
                config.max_key_length,
                true,
            ),
        }
    }

    /// Recomputes the local message_info_cache after bootstrap or loading the state from disk
    pub fn recompute_message_info_cache(&mut self) {
        self.message_info_cache.clear();

        let db = self.db.read();

        // Iterates over the whole database
        let mut last_id: Option<Vec<u8>> = None;

        while let Some((serialized_message_id, _)) = match last_id {
            Some(id) => db
                .iterator_cf(
                    STATE_CF,
                    MassaIteratorMode::From(&can_be_executed_key!(id), MassaDirection::Forward),
                )
                .nth(1),
            None => db
                .prefix_iterator_cf(STATE_CF, ASYNC_POOL_PREFIX.as_bytes())
                .next(),
        } {
            if !serialized_message_id.starts_with(ASYNC_POOL_PREFIX.as_bytes()) {
                break;
            }

            let (_, message_id) = self
                .message_id_deserializer
                .deserialize::<DeserializeError>(&serialized_message_id[ASYNC_POOL_PREFIX.len()..])
                .expect(MESSAGE_ID_DESER_ERROR);

            if let Some(message) = self.fetch_message(&message_id) {
                self.message_info_cache.insert(message_id, message.into());
            }

            // The -1 is to remove the IDENT byte at the end of the key
            last_id = Some(
                serialized_message_id[ASYNC_POOL_PREFIX.len()..serialized_message_id.len() - 1]
                    .to_vec(),
            );
        }
    }

    /// Resets the pool to its initial state
    ///
    /// USED ONLY FOR BOOTSTRAP
    pub fn reset(&mut self) {
        self.db
            .write()
            .delete_prefix(ASYNC_POOL_PREFIX, STATE_CF, None);
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

    /// Query a message from the database.
    ///
    /// This should only be called when we know we want to execute the message.
    /// Otherwise, we should use the `message_info_cache`.
    pub fn fetch_message(&self, message_id: &AsyncMessageId) -> Option<AsyncMessage> {
        let db = self.db.read();

        let mut serialized_message_id = Vec::new();
        self.message_id_serializer
            .serialize(message_id, &mut serialized_message_id)
            .expect(MESSAGE_ID_SER_ERROR);

        let mut serialized_message: Vec<u8> = Vec::new();
        for (serialized_key, serialized_value) in
            db.prefix_iterator_cf(STATE_CF, &message_id_prefix!(serialized_message_id))
        {
            if !serialized_key.starts_with(&message_id_prefix!(serialized_message_id)) {
                break;
            }

            serialized_message.extend(serialized_value.iter());
        }

        match self
            .message_deserializer_db
            .deserialize::<DeserializeError>(&serialized_message)
        {
            Ok((_, message)) => Some(message),
            _ => None,
        }
    }

    /// Query a vec of messages from the database.
    ///
    /// This should only be called when we know we want to execute the messages.
    /// Otherwise, we should use the `message_info_cache`.
    pub fn fetch_messages<'a>(
        &self,
        message_ids: Vec<&'a AsyncMessageId>,
    ) -> Vec<(&'a AsyncMessageId, Option<AsyncMessage>)> {
        let mut fetched_messages = Vec::new();

        for message_id in message_ids.iter() {
            let message = self.fetch_message(message_id);
            fetched_messages.push((*message_id, message));
        }

        fetched_messages
    }

    /// Deserializes the key and value, useful after bootstrap
    pub fn is_key_value_valid(&self, serialized_key: &[u8], serialized_value: &[u8]) -> bool {
        if !serialized_key.starts_with(ASYNC_POOL_PREFIX.as_bytes()) {
            return false;
        }

        let Ok((rest, _id)) = self
            .message_id_deserializer
            .deserialize::<DeserializeError>(&serialized_key[ASYNC_POOL_PREFIX.len()..])
        else {
            return false;
        };
        if rest.len() != 1 {
            return false;
        }

        match rest[0] {
            EMISSION_SLOT_IDENT => {
                let Ok((rest, _value)) = self
                    .message_deserializer_db
                    .slot_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            EMISSION_INDEX_IDENT => {
                let Ok((rest, _value)) = self
                    .message_deserializer_db
                    .emission_index_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            SENDER_IDENT => {
                let Ok((rest, _value)): std::result::Result<
                    (&[u8], Address),
                    nom::Err<massa_serialization::DeserializeError<'_>>,
                > = self
                    .message_deserializer_db
                    .address_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            DESTINATION_IDENT => {
                let Ok((rest, _value)): std::result::Result<
                    (&[u8], Address),
                    nom::Err<massa_serialization::DeserializeError<'_>>,
                > = self
                    .message_deserializer_db
                    .address_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            FUNCTION_IDENT => {
                let Some(len) = serialized_value.first() else {
                    return false;
                };

                if serialized_value.len() != *len as usize + 1 {
                    return false;
                }

                let Ok(_value) = String::from_utf8(serialized_value[1..].to_vec()) else {
                    return false;
                };
            }
            MAX_GAS_IDENT => {
                let Ok((rest, _value)) = self
                    .message_deserializer_db
                    .max_gas_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            FEE_IDENT => {
                let Ok((rest, _value)) = self
                    .message_deserializer_db
                    .amount_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            COINS_IDENT => {
                let Ok((rest, _value)) = self
                    .message_deserializer_db
                    .amount_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            VALIDITY_START_IDENT => {
                let Ok((rest, _value)) = self
                    .message_deserializer_db
                    .slot_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            VALIDITY_END_IDENT => {
                let Ok((rest, _value)) = self
                    .message_deserializer_db
                    .slot_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            FUNCTION_PARAMS_IDENT => {
                let Ok((rest, _value)) = self
                    .message_deserializer_db
                    .function_params_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            TRIGGER_IDENT => {
                let Ok((rest, _value)) = self
                    .message_deserializer_db
                    .trigger_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            CAN_BE_EXECUTED_IDENT => {
                let Ok((rest, _value)) = self
                    .message_deserializer_db
                    .bool_deserializer
                    .deserialize::<DeserializeError>(serialized_value)
                else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            _ => {
                return false;
            }
        }

        true
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
        max_function_length: u16,
        max_parameters_length: u64,
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
                max_function_length,
                max_parameters_length,
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
        db.put_or_update_entry_value(
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
        db.put_or_update_entry_value(
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
        db.put_or_update_entry_value(
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
        db.put_or_update_entry_value(
            batch,
            destination_key!(serialized_message_id),
            &serialized_destination,
        );

        // Function
        let mut serialized_function = Vec::new();
        self.message_serializer
            .function_serializer
            .serialize(&message.function, &mut serialized_function)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(
            batch,
            function_key!(serialized_message_id),
            &serialized_function,
        );

        // Max gas
        let mut serialized_max_gas = Vec::new();
        self.message_serializer
            .u64_serializer
            .serialize(&message.max_gas, &mut serialized_max_gas)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(
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
        db.put_or_update_entry_value(batch, fee_key!(serialized_message_id), &serialized_fee);

        // Coins
        let mut serialized_coins = Vec::new();
        self.message_serializer
            .amount_serializer
            .serialize(&message.coins, &mut serialized_coins)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(batch, coins_key!(serialized_message_id), &serialized_coins);

        // Validity start
        let mut serialized_validity_start = Vec::new();
        self.message_serializer
            .slot_serializer
            .serialize(&message.validity_start, &mut serialized_validity_start)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(
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
        db.put_or_update_entry_value(
            batch,
            validity_end_key!(serialized_message_id),
            &serialized_validity_end,
        );

        // Params
        let mut serialized_params = Vec::new();
        self.message_serializer
            .function_params_serializer
            .serialize(&message.function_params, &mut serialized_params)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(
            batch,
            function_params_key!(serialized_message_id),
            &serialized_params,
        );

        // Trigger
        let mut serialized_trigger = Vec::new();
        self.message_serializer
            .trigger_serializer
            .serialize(&message.trigger, &mut serialized_trigger)
            .expect(MESSAGE_SER_ERROR);
        db.put_or_update_entry_value(
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
        db.put_or_update_entry_value(
            batch,
            can_be_executed_key!(serialized_message_id),
            &serialized_can_be_executed,
        );
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
            db.put_or_update_entry_value(
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
            db.put_or_update_entry_value(
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
            db.put_or_update_entry_value(
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
            db.put_or_update_entry_value(
                batch,
                destination_key!(serialized_message_id),
                &serialized_destination,
            );
        }

        // Function name
        if let SetOrKeep::Set(function) = message_update.function {
            let mut serialized_function = Vec::new();
            self.message_serializer
                .function_serializer
                .serialize(&function, &mut serialized_function)
                .expect(MESSAGE_SER_ERROR);
            db.put_or_update_entry_value(
                batch,
                function_key!(serialized_message_id),
                &serialized_function,
            );
        }

        // Max gas
        if let SetOrKeep::Set(max_gas) = message_update.max_gas {
            let mut serialized_max_gas = Vec::new();
            self.message_serializer
                .u64_serializer
                .serialize(&max_gas, &mut serialized_max_gas)
                .expect(MESSAGE_SER_ERROR);
            db.put_or_update_entry_value(
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
            db.put_or_update_entry_value(batch, fee_key!(serialized_message_id), &serialized_fee);
        }

        // Coins
        if let SetOrKeep::Set(coins) = message_update.coins {
            let mut serialized_coins = Vec::new();
            self.message_serializer
                .amount_serializer
                .serialize(&coins, &mut serialized_coins)
                .expect(MESSAGE_SER_ERROR);
            db.put_or_update_entry_value(
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
            db.put_or_update_entry_value(
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
            db.put_or_update_entry_value(
                batch,
                validity_end_key!(serialized_message_id),
                &serialized_validity_end,
            );
        }

        // Params
        if let SetOrKeep::Set(params) = message_update.function_params {
            let mut serialized_function_params = Vec::new();
            self.message_serializer
                .function_params_serializer
                .serialize(&params, &mut serialized_function_params)
                .expect(MESSAGE_SER_ERROR);
            db.put_or_update_entry_value(
                batch,
                function_params_key!(serialized_message_id),
                &serialized_function_params,
            );
        }

        // Trigger
        if let SetOrKeep::Set(trigger) = message_update.trigger {
            let mut serialized_trigger = Vec::new();
            self.message_serializer
                .trigger_serializer
                .serialize(&trigger, &mut serialized_trigger)
                .expect(MESSAGE_SER_ERROR);
            db.put_or_update_entry_value(
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
            db.put_or_update_entry_value(
                batch,
                can_be_executed_key!(serialized_message_id),
                &serialized_can_be_executed,
            );
        }
    }

    /// Delete every sub-entry associated to the given address.
    ///
    /// # Arguments
    /// * batch: the given operation batch to update
    fn delete_entry(&self, message_id: &AsyncMessageId, batch: &mut DBBatch) {
        let db = self.db.read();
        let mut serialized_message_id = Vec::new();
        self.message_id_serializer
            .serialize(message_id, &mut serialized_message_id)
            .expect(MESSAGE_ID_SER_ERROR);

        db.delete_key(batch, emission_slot_key!(serialized_message_id));
        db.delete_key(batch, emission_index_key!(serialized_message_id));
        db.delete_key(batch, sender_key!(serialized_message_id));
        db.delete_key(batch, destination_key!(serialized_message_id));
        db.delete_key(batch, function_key!(serialized_message_id));
        db.delete_key(batch, max_gas_key!(serialized_message_id));
        db.delete_key(batch, fee_key!(serialized_message_id));
        db.delete_key(batch, coins_key!(serialized_message_id));
        db.delete_key(batch, validity_start_key!(serialized_message_id));
        db.delete_key(batch, validity_end_key!(serialized_message_id));
        db.delete_key(batch, function_params_key!(serialized_message_id));
        db.delete_key(batch, trigger_key!(serialized_message_id));
        db.delete_key(batch, can_be_executed_key!(serialized_message_id));
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;

    use massa_db_exports::{MassaDBConfig, MassaDBController};
    use massa_models::config::{
        MAX_ASYNC_POOL_LENGTH, MAX_DATASTORE_KEY_LENGTH, MAX_FUNCTION_NAME_LENGTH,
        MAX_PARAMETERS_SIZE, THREAD_COUNT,
    };
    use massa_models::{address::Address, amount::Amount, slot::Slot};

    use crate::message::AsyncMessageTrigger;

    use massa_db_worker::MassaDB;
    use parking_lot::RwLock;
    use tempfile::tempdir;

    use super::*;

    fn dump_column(
        db: Arc<RwLock<Box<dyn MassaDBController>>>,
        column: &str,
    ) -> BTreeMap<Vec<u8>, Vec<u8>> {
        db.read()
            .iterator_cf(column, MassaIteratorMode::Start)
            // .collect::<BTreeMap<Vec<u8>, Vec<u8>>>()
            .collect()
    }

    fn create_message() -> AsyncMessage {
        AsyncMessage::new(
            Slot::new(1, 0),
            0,
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
            String::from("test"),
            10000000,
            Amount::from_str("1").unwrap(),
            Amount::from_str("1").unwrap(),
            Slot::new(2, 0),
            Slot::new(3, 0),
            vec![1, 2, 3, 4],
            Some(AsyncMessageTrigger {
                address: Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x")
                    .unwrap(),
                datastore_key: Some(vec![1, 2, 3, 4]),
            }),
            None,
        )
    }

    #[test]
    fn test_pool_ser_deser_empty() {
        let config = AsyncPoolConfig::default();
        let temp_dir = tempdir().expect("Unable to create a temp folder");
        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 100,
        };
        let db: ShareableMassaDBController = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>,
        ));
        let pool = AsyncPool::new(config, db);

        let mut serialized = Vec::new();
        let serializer = AsyncPoolSerializer::new();
        let deserializer = AsyncPoolDeserializer::new(
            THREAD_COUNT,
            MAX_ASYNC_POOL_LENGTH,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE as u64,
            MAX_DATASTORE_KEY_LENGTH as u32,
        );

        let message_ids: Vec<&AsyncMessageId> = vec![];
        let to_ser_ = pool.fetch_messages(message_ids);
        let to_ser = to_ser_
            .iter()
            .map(|(k, v)| (*(*k), v.clone().unwrap()))
            .collect();
        serializer.serialize(&to_ser, &mut serialized).unwrap();

        let (rest, changes_deser) = deserializer
            .deserialize::<DeserializeError>(&serialized)
            .unwrap();
        assert!(rest.is_empty());
        assert_eq!(to_ser, changes_deser);
    }

    #[test]
    fn test_pool_ser_deser() {
        let config = AsyncPoolConfig::default();
        let temp_dir = tempdir().expect("Unable to create a temp folder");
        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 100,
        };
        let db: ShareableMassaDBController = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>,
        ));
        let pool = AsyncPool::new(config, db);

        let mut serialized = Vec::new();
        let serializer = AsyncPoolSerializer::new();
        let deserializer = AsyncPoolDeserializer::new(
            THREAD_COUNT,
            MAX_ASYNC_POOL_LENGTH,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE as u64,
            MAX_DATASTORE_KEY_LENGTH as u32,
        );

        let message = create_message();
        let message_id = message.compute_id();
        let mut message2 = message.clone();
        message2.emission_index += 1; // update AsyncMessageId
        message2.function = "test2".to_string();
        let message2_id = message2.compute_id();
        assert_ne!(message_id, message2_id);

        let mut batch = DBBatch::new();
        pool.put_entry(&message.compute_id(), message.clone(), &mut batch);
        pool.put_entry(&message2.compute_id(), message2.clone(), &mut batch);
        let versioning_batch = DBBatch::new();
        let slot_1 = Slot::new(1, 0);
        pool.db
            .write()
            .write_batch(batch, versioning_batch, Some(slot_1));

        let message_ids: Vec<&AsyncMessageId> = vec![&message_id, &message2_id];
        let to_ser_ = pool.fetch_messages(message_ids);
        let to_ser = to_ser_
            .iter()
            .map(|(k, v)| (*(*k), v.clone().unwrap()))
            .collect();
        serializer.serialize(&to_ser, &mut serialized).unwrap();
        assert_eq!(to_ser.len(), 2);

        let (rest, changes_deser) = deserializer
            .deserialize::<DeserializeError>(&serialized)
            .unwrap();
        assert!(rest.is_empty());
        assert_eq!(to_ser, changes_deser);
    }

    #[test]
    fn test_pool_ser_deser_too_high() {
        // Ser 2 msg but deserializer could only handle 1

        let config = AsyncPoolConfig::default();
        let temp_dir = tempdir().expect("Unable to create a temp folder");
        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 100,
        };
        let db: ShareableMassaDBController = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>,
        ));
        let pool = AsyncPool::new(config, db);

        let mut serialized = Vec::new();
        let serializer = AsyncPoolSerializer::new();
        let deserializer = AsyncPoolDeserializer::new(
            THREAD_COUNT,
            1,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE as u64,
            MAX_DATASTORE_KEY_LENGTH as u32,
        );

        let message = create_message();
        let message_id = message.compute_id();
        let mut message2 = message.clone();
        message2.emission_index += 1; // update AsyncMessageId
        message2.function = "test2".to_string();
        let message2_id = message2.compute_id();
        let mut batch = DBBatch::new();
        pool.put_entry(&message.compute_id(), message.clone(), &mut batch);
        pool.put_entry(&message2.compute_id(), message2.clone(), &mut batch);
        let versioning_batch = DBBatch::new();
        let slot_1 = Slot::new(1, 0);
        pool.db
            .write()
            .write_batch(batch, versioning_batch, Some(slot_1));

        let message_ids: Vec<&AsyncMessageId> = vec![&message_id, &message2_id];
        let to_ser_ = pool.fetch_messages(message_ids);
        let to_ser = to_ser_
            .iter()
            .map(|(k, v)| (*(*k), v.clone().unwrap()))
            .collect();
        serializer.serialize(&to_ser, &mut serialized).unwrap();
        assert_eq!(to_ser.len(), 2);

        let res = deserializer.deserialize::<DeserializeError>(&serialized);
        assert!(res.is_err());
    }

    #[test]
    fn test_pool_entry() {
        // Test update_entry & delete_entry

        let config = AsyncPoolConfig::default();
        let temp_dir = tempdir().expect("Unable to create a temp folder");
        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 100,
        };
        let db: ShareableMassaDBController = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>,
        ));
        let pool = AsyncPool::new(config, db);

        let message = create_message();
        let message_id = message.compute_id();
        let mut message2 = message.clone();
        message2.emission_index += 1; // update AsyncMessageId
        message2.function = "test2".to_string();
        let message2_id = message2.compute_id();
        let mut batch = DBBatch::new();
        pool.put_entry(&message_id, message.clone(), &mut batch);
        pool.put_entry(&message2_id, message2.clone(), &mut batch);

        let versioning_batch = DBBatch::new();
        let slot_1 = Slot::new(1, 0);
        pool.db
            .write()
            .write_batch(batch, versioning_batch, Some(slot_1));

        let content = dump_column(pool.db.clone(), "state");
        assert_eq!(content.len(), 26); // 2 entries added, split in 13 prefix

        let mut batch2 = DBBatch::new();
        pool.delete_entry(&message_id, &mut batch2);
        let message_update = AsyncMessageUpdate {
            function: SetOrKeep::Set("test0".to_string()),
            ..Default::default()
        };
        pool.update_entry(&message2_id, message_update, &mut batch2);

        let versioning_batch2 = DBBatch::new();
        let slot_2 = Slot::new(2, 0);
        pool.db
            .write()
            .write_batch(batch2, versioning_batch2, Some(slot_2));

        let content = dump_column(pool.db.clone(), "state");
        assert_eq!(content.len(), 13);
    }

    #[test]
    fn test_pool_cache_grow() {
        // Init a pool, add changes and check the internal cache grows accordingly
        // Reset it and check the cache is empty

        let config = AsyncPoolConfig::default();
        let temp_dir = tempdir().expect("Unable to create a temp folder");
        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 100,
        };
        let db: ShareableMassaDBController = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>,
        ));
        let mut pool = AsyncPool::new(config, db);

        assert!(pool.message_info_cache.is_empty());

        let message = create_message();
        let message_id = message.compute_id();

        let mut changes = AsyncPoolChanges::default();
        changes
            .0
            .insert(message_id, SetUpdateOrDelete::Set(message.clone()));

        const EXPECT_CACHE_COUNT: u64 = 100;
        for i in 0..EXPECT_CACHE_COUNT {
            let mut message2 = message.clone();
            message2.fee = Amount::from_raw(i);
            assert_ne!(message.compute_id(), message2.compute_id());

            changes
                .0
                .insert(message2.compute_id(), SetUpdateOrDelete::Set(message2));
        }

        let mut batch = DBBatch::new();
        pool.apply_changes_to_batch(&changes, &mut batch);
        assert_eq!(pool.message_info_cache.len() as u64, EXPECT_CACHE_COUNT + 1);

        pool.reset();
        assert!(pool.message_info_cache.is_empty());
    }

    #[test]
    fn test_pool_recompute_cache() {
        // Init a pool, add changes
        // drop pool
        // Init another pool (will read db from disk), recompute cache and cmp with original cache

        let config = AsyncPoolConfig::default();
        let temp_dir = tempdir().expect("Unable to create a temp folder");
        let db_config = MassaDBConfig {
            path: temp_dir.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 100,
        };
        let db: ShareableMassaDBController = Arc::new(RwLock::new(Box::new(MassaDB::new(
            db_config.clone(),
        ))
            as Box<(dyn MassaDBController + 'static)>));
        let mut pool = AsyncPool::new(config.clone(), db);

        assert!(pool.message_info_cache.is_empty());

        let message = create_message();
        let message_id = message.compute_id();

        let mut changes = AsyncPoolChanges::default();
        changes
            .0
            .insert(message_id, SetUpdateOrDelete::Set(message.clone()));

        const EXPECT_CACHE_COUNT: u64 = 100;
        for i in 0..EXPECT_CACHE_COUNT {
            let mut message2 = message.clone();
            message2.fee = Amount::from_raw(i);
            assert_ne!(message.compute_id(), message2.compute_id());

            changes
                .0
                .insert(message2.compute_id(), SetUpdateOrDelete::Set(message2));
        }

        let mut batch = DBBatch::new();
        pool.apply_changes_to_batch(&changes, &mut batch);
        assert_eq!(pool.message_info_cache.len() as u64, EXPECT_CACHE_COUNT + 1);

        let message_info_cache1 = pool.message_info_cache.clone();

        let versioning_batch = DBBatch::new();
        let slot_1 = Slot::new(1, 0);
        pool.db
            .write()
            .write_batch(batch, versioning_batch, Some(slot_1));

        drop(pool);

        let db2: ShareableMassaDBController = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>,
        ));
        let mut pool2 = AsyncPool::new(config, db2);

        pool2.recompute_message_info_cache();

        assert_eq!(pool2.message_info_cache, message_info_cache1);
    }
}
