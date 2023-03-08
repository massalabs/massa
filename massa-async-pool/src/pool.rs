//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a finite size final pool of asynchronous messages for use in the context of autonomous smart contracts

use crate::{
    changes::{AsyncPoolChanges, Change},
    config::AsyncPoolConfig,
    message::{AsyncMessage, AsyncMessageId},
    AsyncMessageDeserializer, AsyncMessageIdDeserializer, AsyncMessageIdSerializer,
    AsyncMessageSerializer, AsyncMessageTrigger,
};
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_ledger_exports::LedgerChanges;
use massa_models::{slot::Slot, streaming_step::StreamingStep};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Included, Unbounded};

const ASYNC_POOL_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

/// Represents a pool of sorted messages in a deterministic way.
/// The final asynchronous pool is attached to the output of the latest final slot within the context of massa-final-state.
/// Nodes must bootstrap the final message pool when they join the network.
#[derive(Clone)]
pub struct AsyncPool {
    /// Asynchronous pool configuration
    config: AsyncPoolConfig,

    /// Messages sorted by decreasing ID (decreasing priority)
    pub messages: BTreeMap<AsyncMessageId, AsyncMessage>,

    /// Hash of the asynchronous pool
    pub hash: Hash,
}

impl AsyncPool {
    /// Creates an empty `AsyncPool`
    pub fn new(config: AsyncPoolConfig) -> AsyncPool {
        AsyncPool {
            config,
            messages: Default::default(),
            hash: Hash::from_bytes(ASYNC_POOL_HASH_INITIAL_BYTES),
        }
    }

    /// Creates an `AsyncPool` from an existing snapshot
    pub fn from_snapshot(
        config: AsyncPoolConfig,
        messages: BTreeMap<AsyncMessageId, AsyncMessage>,
        hash: Hash,
    ) -> AsyncPool {
        AsyncPool {
            config,
            messages,
            hash,
        }
    }

    /// Resets the pool to its initial state
    ///
    /// USED ONLY FOR BOOTSTRAP
    pub fn reset(&mut self) {
        self.messages.clear();
        self.hash = Hash::from_bytes(ASYNC_POOL_HASH_INITIAL_BYTES);
    }

    /// Applies pre-compiled `AsyncPoolChanges` to the pool without checking for overflows.
    /// This function is used when applying pre-compiled `AsyncPoolChanges` to an `AsyncPool`.
    ///
    /// # arguments
    /// * `changes`: `AsyncPoolChanges` listing all asynchronous pool changes (message insertions/deletions)
    pub fn apply_changes_unchecked(&mut self, changes: &AsyncPoolChanges) {
        for change in changes.0.iter() {
            match change {
                // add a new message to the pool
                Change::Add(message_id, message) => {
                    if self.messages.insert(*message_id, message.clone()).is_none() {
                        self.hash ^= message.hash;
                    }
                }

                Change::Activate(message_id) => {
                    if let Some(message) = self.messages.get_mut(message_id) {
                        self.hash ^= message.hash;
                        message.can_be_executed = true;
                        message.compute_hash();
                        self.hash ^= message.hash;
                    }
                }

                // delete a message from the pool
                Change::Delete(message_id) => {
                    if let Some(removed_message) = self.messages.remove(message_id) {
                        self.hash ^= removed_message.hash;
                    }
                }
            }
        }
    }

    /// Settles a slot, adding new messages to the pool and returning expired and excess ones.
    /// This method is called at the end of a slot execution to apply the list of emitted messages,
    /// and get the list of pruned messages for `coins` reimbursement.
    ///
    /// # arguments
    /// * `slot`: used to filter out expired messages, not stored
    /// * `new_messages`: list of `AsyncMessage` to add to the pool
    ///
    /// # returns
    /// The list of `(message_id, message)` that were eliminated from the pool after the changes were applied, sorted in the following order:
    /// * expired messages from the pool, in priority order (from highest to lowest priority)
    /// * expired messages from `new_messages` (in the order they appear in `new_messages`)
    /// * excess messages after inserting all remaining `new_messages`, in priority order (from highest to lowest priority)
    /// The list of message that their trigger has been triggered.
    #[allow(clippy::type_complexity)]
    pub fn settle_slot(
        &mut self,
        slot: &Slot,
        new_messages: &mut Vec<(AsyncMessageId, AsyncMessage)>,
        ledger_changes: &LedgerChanges,
    ) -> (
        Vec<(AsyncMessageId, AsyncMessage)>,
        Vec<(AsyncMessageId, AsyncMessage)>,
    ) {
        // Filter out all messages for which the validity end is expired.
        // Note that the validity_end bound is NOT included in the validity interval of the message.
        let mut eliminated: Vec<_> = self
            .messages
            .drain_filter(|_k, v| *slot >= v.validity_end)
            .chain(new_messages.drain_filter(|(_k, v)| *slot >= v.validity_end))
            .collect();

        // Insert new messages into the pool
        self.messages.extend(new_messages.clone());

        // Truncate message pool to its max size, removing non-prioritary items
        let excess_count = self
            .messages
            .len()
            .saturating_sub(self.config.max_length as usize);
        eliminated.reserve_exact(excess_count);
        for _ in 0..excess_count {
            eliminated.push(self.messages.pop_last().unwrap()); // will not panic (checked at excess_count computation)
        }
        let mut triggered = Vec::new();
        for (id, message) in self.messages.iter_mut() {
            if let Some(filter) = &message.trigger && !message.can_be_executed && is_triggered(filter, ledger_changes)
            {
                message.can_be_executed = true;
                triggered.push((*id, message.clone()));
            }
        }
        (eliminated, triggered)
    }

    /// Takes the best possible batch of messages to execute, with gas limits and slot validity filtering.
    /// The returned messages are removed from the pool.
    /// This method is used at the beginning of a slot execution to list asynchronous messages to execute.
    ///
    /// # arguments
    /// * `slot`: select only messages that are valid within this slot
    /// * `available_gas`: maximum amount of available gas
    ///
    /// # returns
    /// A vector of messages, sorted from the most priority to the least priority
    pub fn take_batch_to_execute(
        &mut self,
        slot: Slot,
        mut available_gas: u64,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        // gather all selected items and remove them from self.messages
        // iterate in decreasing priority order
        self.messages
            .drain_filter(|_, message| {
                // check available gas and validity period
                if available_gas >= message.max_gas
                    && slot >= message.validity_start
                    && slot < message.validity_end
                    && message.can_be_executed
                {
                    available_gas -= message.max_gas;
                    true
                } else {
                    false
                }
            })
            .collect()
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
        let mut pool_part = BTreeMap::new();
        let left_bound = match cursor {
            StreamingStep::Started => Unbounded,
            StreamingStep::Ongoing(last_id) => Excluded(last_id),
            StreamingStep::Finished(_) => return (pool_part, cursor),
        };
        let mut pool_part_last_id: Option<AsyncMessageId> = None;
        for (id, message) in self.messages.range((left_bound, Unbounded)) {
            if pool_part.len() < self.config.bootstrap_part_size as usize {
                pool_part.insert(*id, message.clone());
                pool_part_last_id = Some(*id);
            } else {
                break;
            }
        }
        if let Some(last_id) = pool_part_last_id {
            (pool_part, StreamingStep::Ongoing(last_id))
        } else {
            (pool_part, StreamingStep::Finished(None))
        }
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
        for (message_id, message) in part {
            if self.messages.insert(message_id, message.clone()).is_none() {
                self.hash ^= message.hash;
            }
        }
        if let Some(message_id) = self.messages.last_key_value().map(|(&id, _)| id) {
            StreamingStep::Ongoing(message_id)
        } else {
            StreamingStep::Finished(None)
        }
    }
}

/// Check in the ledger changes if a message trigger has been triggered
fn is_triggered(filter: &AsyncMessageTrigger, ledger_changes: &LedgerChanges) -> bool {
    ledger_changes.has_changes(&filter.address, filter.datastore_key.clone())
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
            async_message_serializer: AsyncMessageSerializer::new(),
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
    async_message_deserializer: AsyncMessageDeserializer,
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
            async_message_deserializer: AsyncMessageDeserializer::new(
                thread_count,
                max_async_message_data,
                max_key_length,
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
                        self.async_message_deserializer.deserialize(input)
                    }),
                )),
            ),
        )
        .map(|vec| vec.into_iter().collect())
        .parse(buffer)
    }
}

#[test]
fn test_take_batch() {
    use massa_hash::Hash;
    use massa_models::{
        address::{Address, UserAddress},
        amount::Amount,
        slot::Slot,
    };
    use std::str::FromStr;

    let config = AsyncPoolConfig {
        thread_count: 2,
        max_length: 10,
        max_async_message_data: 1_000_000,
        bootstrap_part_size: 100,
    };
    let mut pool = AsyncPool::new(config);
    let address = Address::User(UserAddress(Hash::compute_from(b"abc")));
    for i in 1..10 {
        let message = AsyncMessage::new_with_hash(
            Slot::new(0, 0),
            0,
            address,
            address,
            "function".to_string(),
            i,
            Amount::from_str("0.1").unwrap(),
            Amount::from_str("0.3").unwrap(),
            Slot::new(1, 0),
            Slot::new(3, 0),
            Vec::new(),
            None,
        );
        pool.messages.insert(message.compute_id(), message);
    }
    assert_eq!(pool.messages.len(), 9);
    pool.take_batch_to_execute(Slot::new(2, 0), 19);
    assert_eq!(pool.messages.len(), 4);
}
