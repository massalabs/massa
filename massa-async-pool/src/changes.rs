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

///! Copyright (c) 2022 MASSA LABS <info@massa.net>

///! This file provides structures representing changes to the asynchronous message pool
use crate::{
    message::{AsyncMessage, AsyncMessageId, AsyncMessageIdDeserializer, AsyncMessageIdSerializer},
    AsyncMessageDeserializer, AsyncMessageSerializer,
};

/// Enum representing a value U with identifier T being added or deleted
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change<T, U> {
    /// an item with identifier T and value U is added
    Add(T, U),

    /// an item with identifier T is deleted
    Delete(T),
}

/// represents a list of additions and deletions to the asynchronous message pool
#[derive(Default, Debug, Clone, PartialEq, Eq)]
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
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::Serializer;
    /// use massa_models::{Address, Amount, Slot};
    /// use std::str::FromStr;
    /// use massa_async_pool::{AsyncMessage, Change, AsyncPoolChanges, AsyncPoolChangesSerializer};
    ///
    /// let message = AsyncMessage {
    ///     emission_slot: Slot::new(1, 0),
    ///     emission_index: 0,
    ///     sender:  Address::from_str("A12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///     destination: Address::from_str("A12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
    ///     handler: String::from("test"),
    ///     max_gas: 10000000,
    ///     gas_price: Amount::from_str("1").unwrap(),
    ///     coins: Amount::from_str("1").unwrap(),
    ///     validity_start: Slot::new(2, 0),
    ///     validity_end: Slot::new(3, 0),
    ///     data: vec![1, 2, 3, 4]
    /// };
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
                    buffer.push(0);
                    self.id_serializer.serialize(id, buffer)?;
                    self.message_serializer.serialize(message, buffer)?;
                }
                Change::Delete(id) => {
                    buffer.push(1);
                    self.id_serializer.serialize(id, buffer)?;
                }
            }
        }
        Ok(())
    }
}

pub struct AsyncPoolChangesDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    id_deserializer: AsyncMessageIdDeserializer,
    message_deserializer: AsyncMessageDeserializer,
}

impl AsyncPoolChangesDeserializer {
    pub fn new() -> Self {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(1000000)),
            id_deserializer: AsyncMessageIdDeserializer::new(),
            message_deserializer: AsyncMessageDeserializer::new(),
        }
    }
}

impl Default for AsyncPoolChangesDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<AsyncPoolChanges> for AsyncPoolChangesDeserializer {
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_models::{Address, Amount, Slot};
    /// use std::str::FromStr;
    /// use massa_async_pool::{AsyncMessage, Change, AsyncPoolChanges, AsyncPoolChangesSerializer, AsyncPoolChangesDeserializer};
    ///
    /// let message = AsyncMessage {
    ///     emission_slot: Slot::new(1, 0),
    ///     emission_index: 0,
    ///     sender:  Address::from_str("A12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///     destination: Address::from_str("A12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
    ///     handler: String::from("test"),
    ///     max_gas: 10000000,
    ///     gas_price: Amount::from_str("1").unwrap(),
    ///     coins: Amount::from_str("1").unwrap(),
    ///     validity_start: Slot::new(2, 0),
    ///     validity_end: Slot::new(3, 0),
    ///     data: vec![1, 2, 3, 4]
    /// };
    /// let changes: AsyncPoolChanges = AsyncPoolChanges(vec![Change::Add(message.compute_id(), message)]);
    /// let mut serialized = Vec::new();
    /// let serializer = AsyncPoolChangesSerializer::new();
    /// let deserializer = AsyncPoolChangesDeserializer::new();
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
                    self.u64_deserializer.deserialize(input)
                }),
                |input: &'a [u8]| match input.first() {
                    Some(0) => {
                        let (rest, (id, message)) = tuple((
                            |input| self.id_deserializer.deserialize(input),
                            |input| self.message_deserializer.deserialize(input),
                        ))(&input[1..])?;
                        Ok((rest, Change::Add(id, message)))
                    }
                    Some(1) => {
                        let (rest, id) = self.id_deserializer.deserialize(&input[1..])?;
                        Ok((rest, Change::Delete(id)))
                    }
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
}
