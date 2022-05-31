use std::ops::Bound::Included;

use massa_models::{
    DeserializeCompact, SerializeCompact, U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::{error::context, multi::length_count, IResult};

///! Copyright (c) 2022 MASSA LABS <info@massa.net>

///! This file provides structures representing changes to the asynchronous message pool
use crate::message::{
    AsyncMessage, AsyncMessageId, AsyncMessageIdDeserializer, AsyncMessageIdSerializer,
};

/// Enum representing a value U with identifier T being added or deleted
#[derive(Debug, Clone)]
pub enum Change<T, U> {
    /// an item with identifier T and value U is added
    Add(T, U),

    /// an item with identifier T is deleted
    Delete(T),
}

/// represents a list of additions and deletions to the asynchronous message pool
#[derive(Default, Debug, Clone)]
pub struct AsyncPoolChanges(pub Vec<Change<AsyncMessageId, AsyncMessage>>);

/// `AsyncPoolChanges` serializer
pub struct AsyncPoolChangesSerializer {
    u64_serializer: U64VarIntSerializer,
    id_serializer: AsyncMessageIdSerializer,
}

impl AsyncPoolChangesSerializer {
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(Included(u64::MIN), Included(u64::MAX)),
            id_serializer: AsyncMessageIdSerializer::new(),
        }
    }
}

impl Default for AsyncPoolChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<AsyncPoolChanges> for AsyncPoolChangesSerializer {
    fn serialize(&self, value: &AsyncPoolChanges) -> Result<Vec<u8>, SerializeError> {
        let mut res = Vec::new();
        res.extend(self.u64_serializer.serialize(
            &(value.0.len().try_into().map_err(|_| {
                SerializeError::GeneralError("Fail to transform usize to u64".to_string())
            })?),
        )?);
        for change in &value.0 {
            match change {
                Change::Add(id, message) => {
                    res.push(0);
                    res.extend(self.id_serializer.serialize(id)?);
                    res.extend(
                        message
                            .to_bytes_compact()
                            .map_err(|err| SerializeError::GeneralError(err.to_string()))?,
                    );
                }
                Change::Delete(id) => {
                    res.push(1);
                    res.extend(self.id_serializer.serialize(id)?);
                }
            }
        }
        Ok(res)
    }
}

pub struct AsyncPoolChangesDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    id_deserializer: AsyncMessageIdDeserializer,
}

impl AsyncPoolChangesDeserializer {
    pub fn new() -> Self {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            id_deserializer: AsyncMessageIdDeserializer::new(),
        }
    }
}

impl Default for AsyncPoolChangesDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<AsyncPoolChanges> for AsyncPoolChangesDeserializer {
    fn deserialize<'a>(&self, buffer: &'a [u8]) -> IResult<&'a [u8], AsyncPoolChanges> {
        let mut parser = length_count(
            context("length in AsyncPoolChanges", |input| {
                self.u64_deserializer.deserialize(input)
            }),
            |input: &'a [u8]| {
                match input.get(0) {
                    Some(0) => {
                        let (rest, id) = self.id_deserializer.deserialize(&input[1..])?;
                        let (message, delta) =
                            AsyncMessage::from_bytes_compact(rest).map_err(|_| {
                                nom::Err::Error(nom::error::Error::new(
                                    buffer,
                                    nom::error::ErrorKind::LengthValue,
                                ))
                            })?;
                        // Safe because delta as been increment after dereferencing
                        // Will not be here anymore when async message has new serialize form
                        Ok((&rest[delta..], Change::Add(id, message)))
                    }
                    Some(1) => {
                        let (rest, id) = self.id_deserializer.deserialize(&input[1..])?;
                        Ok((rest, Change::Delete(id)))
                    }
                    Some(_) => Err(nom::Err::Error(nom::error::Error::new(
                        buffer,
                        nom::error::ErrorKind::Digit,
                    ))),
                    None => Err(nom::Err::Error(nom::error::Error::new(
                        buffer,
                        nom::error::ErrorKind::LengthValue,
                    ))),
                }
            },
        );

        parser(buffer).map(|(rest, changes)| (rest, AsyncPoolChanges(changes)))
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
