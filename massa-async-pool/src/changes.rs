use std::{
    collections::{btree_map::Entry, BTreeMap},
    ops::Bound::Included,
};

use massa_ledger_exports::{
    Applicable, SetOrKeep, SetUpdateOrDelete, SetUpdateOrDeleteDeserializer,
    SetUpdateOrDeleteSerializer,
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use serde::{Deserialize, Serialize};

///! Copyright (c) 2022 MASSA LABS <info@massa.net>

///! This file provides structures representing changes to the asynchronous message pool
use crate::{
    message::{
        AsyncMessage, AsyncMessageId, AsyncMessageIdDeserializer, AsyncMessageIdSerializer,
        AsyncMessageUpdate, AsyncMessageUpdateDeserializer, AsyncMessageUpdateSerializer,
    },
    AsyncMessageDeserializer, AsyncMessageSerializer,
};

/*/// Enum representing a value U with identifier T being added or deleted
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Change<T, U> {
    /// an item with identifier T and value U is added
    Add(T, U),

    /// an item with identifier T is ready to be executed
    Activate(T),

    /// an item with identifier T is deleted
    Delete(T),
}*/

/*#[repr(u32)]
enum ChangeId {
    Add = 0,
    Activate = 1,
    Delete = 2,
}*/

/// represents a list of additions and deletions to the asynchronous message pool
#[derive(Default, Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
//pub struct AsyncPoolChanges(pub Vec<Change<AsyncMessageId, AsyncMessage>>);
pub struct AsyncPoolChanges(
    pub BTreeMap<AsyncMessageId, SetUpdateOrDelete<AsyncMessage, AsyncMessageUpdate>>,
);

impl Applicable<AsyncPoolChanges> for AsyncPoolChanges {
    /// extends the current `AsyncPoolChanges` with another one
    fn apply(&mut self, changes: AsyncPoolChanges) {
        for (id, msg_change) in changes.0 {
            match self.0.entry(id) {
                Entry::Occupied(mut occ) => {
                    // apply incoming change if a change on this entry already exists
                    occ.get_mut().apply(msg_change);
                }
                Entry::Vacant(vac) => {
                    // otherwise insert the incoming change
                    vac.insert(msg_change);
                }
            }
        }
    }
}

/// `AsyncPoolChanges` serializer
pub struct AsyncPoolChangesSerializer {
    u64_serializer: U64VarIntSerializer,
    id_serializer: AsyncMessageIdSerializer,
    set_update_or_delete_message_serializer: SetUpdateOrDeleteSerializer<
        AsyncMessage,
        AsyncMessageUpdate,
        AsyncMessageSerializer,
        AsyncMessageUpdateSerializer,
    >,
}

impl AsyncPoolChangesSerializer {
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            id_serializer: AsyncMessageIdSerializer::new(),
            set_update_or_delete_message_serializer: SetUpdateOrDeleteSerializer::new(
                AsyncMessageSerializer::new(false),
                AsyncMessageUpdateSerializer::new(false),
            ),
        }
    }
}

impl Default for AsyncPoolChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<AsyncPoolChanges> for AsyncPoolChangesSerializer {
    /// ## Example
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::Serializer;
    /// use massa_models::{address::Address, amount::Amount, slot::Slot};
    /// use std::str::FromStr;
    /// use massa_async_pool::{AsyncMessage, Change, AsyncPoolChanges, AsyncPoolChangesSerializer};
    ///
    /// let message = AsyncMessage::new_with_hash(
    ///     Slot::new(1, 0),
    ///     0,
    ///     Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///     Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
    ///     String::from("test"),
    ///     10000000,
    ///     Amount::from_str("1").unwrap(),
    ///     Amount::from_str("1").unwrap(),
    ///     Slot::new(2, 0),
    ///     Slot::new(3, 0),
    ///     vec![1, 2, 3, 4],
    ///     None,
    ///     None
    /// );
    /// let changes: AsyncPoolChanges = AsyncPoolChanges(vec![Change::Add(message.compute_id(), message)]);
    /// let mut serialized = Vec::new();
    /// let serializer = AsyncPoolChangesSerializer::new();
    /// serializer.serialize(&changes, &mut serialized).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &AsyncPoolChanges,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.u64_serializer.serialize(
            &(value.0.len().try_into().map_err(|_| {
                SerializeError::GeneralError("Fail to transform usize to u64".to_string())
            })?),
            buffer,
        )?;
        for (id, change) in &value.0 {
            self.id_serializer.serialize(id, buffer)?;
            self.set_update_or_delete_message_serializer
                .serialize(change, buffer)?;
        }
        Ok(())
    }
}

pub struct AsyncPoolChangesDeserializer {
    async_pool_changes_length: U64VarIntDeserializer,
    id_deserializer: AsyncMessageIdDeserializer,
    set_update_or_delete_message_deserializer: SetUpdateOrDeleteDeserializer<
        AsyncMessage,
        AsyncMessageUpdate,
        AsyncMessageDeserializer,
        AsyncMessageUpdateDeserializer,
    >,
}

impl AsyncPoolChangesDeserializer {
    pub fn new(
        thread_count: u8,
        max_async_pool_changes: u64,
        max_async_message_data: u64,
        max_key_length: u32,
    ) -> Self {
        Self {
            async_pool_changes_length: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_async_pool_changes),
            ),
            id_deserializer: AsyncMessageIdDeserializer::new(thread_count),
            set_update_or_delete_message_deserializer: SetUpdateOrDeleteDeserializer::new(
                AsyncMessageDeserializer::new(
                    thread_count,
                    max_async_message_data,
                    max_key_length,
                    false,
                ),
                AsyncMessageUpdateDeserializer::new(
                    thread_count,
                    max_async_message_data,
                    max_key_length,
                    false,
                ),
            ),
        }
    }
}

impl Deserializer<AsyncPoolChanges> for AsyncPoolChangesDeserializer {
    /// ## Example
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_models::{address::Address, amount::Amount, slot::Slot};
    /// use std::str::FromStr;
    /// use massa_async_pool::{AsyncMessage, AsyncMessageTrigger, Change, AsyncPoolChanges, AsyncPoolChangesSerializer, AsyncPoolChangesDeserializer};
    ///
    /// let message = AsyncMessage::new_with_hash(
    ///     Slot::new(1, 0),
    ///     0,
    ///     Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///     Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
    ///     String::from("test"),
    ///     10000000,
    ///     Amount::from_str("1").unwrap(),
    ///     Amount::from_str("1").unwrap(),
    ///     Slot::new(2, 0),
    ///     Slot::new(3, 0),
    ///     vec![1, 2, 3, 4],
    ///     Some(AsyncMessageTrigger {
    ///        address: Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///        datastore_key: Some(vec![1, 2, 3, 4]),
    ///     }),
    ///     None
    /// );
    /// let changes: AsyncPoolChanges = AsyncPoolChanges(vec![Change::Add(message.compute_id(), message.clone()), Change::Delete(message.compute_id())]);
    /// let mut serialized = Vec::new();
    /// let serializer = AsyncPoolChangesSerializer::new();
    /// let deserializer = AsyncPoolChangesDeserializer::new(32, 100000, 100000, 100000);
    /// serializer.serialize(&changes, &mut serialized).unwrap();
    /// let (rest, changes_deser) = deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(changes, changes_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], AsyncPoolChanges, E> {
        context(
            "Failed AsyncPoolChanges deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.async_pool_changes_length.deserialize(input)
                }),
                |input: &'a [u8]| {
                    tuple((
                        context("Failed id deserialization", |input| {
                            self.id_deserializer.deserialize(input)
                        }),
                        context(
                            "Failed set_update_or_delete_message deserialization",
                            |input| {
                                self.set_update_or_delete_message_deserializer
                                    .deserialize(input)
                            },
                        ),
                    ))(input)
                },
            ),
        )
        .map(|vec| AsyncPoolChanges(vec.into_iter().map(|data| (data.0, data.1)).collect()))
        .parse(buffer)
    }
}

impl AsyncPoolChanges {
    /// Extends self with another another `AsyncPoolChanges`.
    /// This simply appends the contents of other to self.
    /// No add/delete compensations are done.
    /*pub fn extend(&mut self, other: AsyncPoolChanges) {
        self.0.extend(other.0);
    }*/

    /// Pushes a message addition to the list of changes.
    /// No add/delete compensations are done.
    ///
    /// Arguments:
    /// * `msg_id`: ID of the message to push as added to the list of changes
    /// * `msg`: message to push as added to the list of changes
    pub fn push_add(&mut self, msg_id: AsyncMessageId, msg: AsyncMessage) {
        let mut change = AsyncPoolChanges::default();
        change.0.insert(msg_id, SetUpdateOrDelete::Set(msg));
        self.apply(change);
    }

    /// Pushes a message deletion to the list of changes.
    /// No add/delete compensations are done.
    ///
    /// Arguments:
    /// * `msg_id`: ID of the message to push as deleted to the list of changes
    pub fn push_delete(&mut self, msg_id: AsyncMessageId) {
        let mut change = AsyncPoolChanges::default();
        change.0.insert(msg_id, SetUpdateOrDelete::Delete);
        self.apply(change);
    }

    /// Pushes a message activation to the list of changes.
    ///
    /// Arguments:
    /// * `msg_id`: ID of the message to push as ready to be executed to the list of changes
    pub fn push_activate(&mut self, msg_id: AsyncMessageId) {
        let mut change = AsyncPoolChanges::default();

        let msg_update = AsyncMessageUpdate {
            can_be_executed: SetOrKeep::Set(true),
            ..Default::default()
        };

        change
            .0
            .insert(msg_id, SetUpdateOrDelete::Update(msg_update));
        self.apply(change);
    }
}
