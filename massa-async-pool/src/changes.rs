use std::ops::Bound::Included;

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
    message::{AsyncMessage, AsyncMessageId, AsyncMessageIdDeserializer, AsyncMessageIdSerializer},
    AsyncMessageDeserializer, AsyncMessageSerializer,
};

/// Enum representing a value U with identifier T being added or deleted
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum Change<T, U> {
    /// an item with identifier T and value U is added
    Add(T, U),

    /// an item with identifier T is ready to be executed
    Activate(T),

    /// an item with identifier T is deleted
    Delete(T),
}

#[repr(u32)]
enum ChangeId {
    Add = 0,
    Activate = 1,
    Delete = 2,
}

/// represents a list of additions and deletions to the asynchronous message pool
#[derive(Default, Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AsyncPoolChanges(pub Vec<Change<AsyncMessageId, AsyncMessage>>);

/// `AsyncPoolChanges` serializer
pub struct AsyncPoolChangesSerializer {
    u64_serializer: U64VarIntSerializer,
    id_serializer: AsyncMessageIdSerializer,
    message_serializer: AsyncMessageSerializer,
}

impl AsyncPoolChangesSerializer {
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            id_serializer: AsyncMessageIdSerializer::new(),
            message_serializer: AsyncMessageSerializer::new(),
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
        for change in &value.0 {
            match change {
                Change::Add(id, message) => {
                    buffer.push(ChangeId::Add as u8);
                    self.id_serializer.serialize(id, buffer)?;
                    self.message_serializer.serialize(message, buffer)?;
                }
                Change::Activate(id) => {
                    buffer.push(ChangeId::Activate as u8);
                    self.id_serializer.serialize(id, buffer)?;
                }
                Change::Delete(id) => {
                    buffer.push(ChangeId::Delete as u8);
                    self.id_serializer.serialize(id, buffer)?;
                }
            }
        }
        Ok(())
    }
}

pub struct AsyncPoolChangesDeserializer {
    async_pool_changes_length: U64VarIntDeserializer,
    id_deserializer: AsyncMessageIdDeserializer,
    message_deserializer: AsyncMessageDeserializer,
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
            message_deserializer: AsyncMessageDeserializer::new(
                thread_count,
                max_async_message_data,
                max_key_length,
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
    ///     })
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
                |input: &'a [u8]| match input.first() {
                    Some(0) => context(
                        "Failed Change::Add deserialization",
                        tuple((
                            context("Failed id deserialization", |input| {
                                self.id_deserializer.deserialize(input)
                            }),
                            context("Failed message deserialization", |input| {
                                self.message_deserializer.deserialize(input)
                            }),
                        )),
                    )
                    .map(|(id, message)| Change::Add(id, message))
                    .parse(&input[1..]),
                    Some(1) => context(
                        "Failed Change::Activate deserialization",
                        context("Failed id deserialization", |input| {
                            self.id_deserializer.deserialize(input)
                        }),
                    )
                    .map(Change::Activate)
                    .parse(&input[1..]),
                    Some(2) => context(
                        "Failed Change::Delete deserialization",
                        context("Failed id deserialization", |input| {
                            self.id_deserializer.deserialize(input)
                        }),
                    )
                    .map(Change::Delete)
                    .parse(&input[1..]),
                    Some(_) => Err(nom::Err::Error(ParseError::from_error_kind(
                        buffer,
                        nom::error::ErrorKind::Digit,
                    ))),
                    None => Err(nom::Err::Error(ParseError::from_error_kind(
                        buffer,
                        nom::error::ErrorKind::LengthValue,
                    ))),
                },
            ),
        )
        .map(AsyncPoolChanges)
        .parse(buffer)
    }
}

impl AsyncPoolChanges {
    /// Extends self with another another `AsyncPoolChanges`.
    /// This simply appends the contents of other to self.
    /// No add/delete compensations are done.
    pub fn extend(&mut self, other: AsyncPoolChanges) {
        self.0.extend(other.0);
    }

    /// Pushes a message addition to the list of changes.
    /// No add/delete compensations are done.
    ///
    /// Arguments:
    /// * `msg_id`: ID of the message to push as added to the list of changes
    /// * `msg`: message to push as added to the list of changes
    pub fn push_add(&mut self, msg_id: AsyncMessageId, msg: AsyncMessage) {
        self.0.push(Change::Add(msg_id, msg));
    }

    /// Pushes a message deletion to the list of changes.
    /// No add/delete compensations are done.
    ///
    /// Arguments:
    /// * `msg_id`: ID of the message to push as deleted to the list of changes
    pub fn push_delete(&mut self, msg_id: AsyncMessageId) {
        self.0.push(Change::Delete(msg_id));
    }

    /// Pushes a message activation to the list of changes.
    ///
    /// Arguments:
    /// * `msg_id`: ID of the message to push as ready to be executed to the list of changes
    pub fn push_activate(&mut self, msg_id: AsyncMessageId) {
        self.0.push(Change::Activate(msg_id));
    }
}
