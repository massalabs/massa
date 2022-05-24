use std::ops::Bound::Included;

use massa_models::{U64VarIntDeserializer, U64VarIntSerializer};
use massa_serialization::Serializer;

///! Copyright (c) 2022 MASSA LABS <info@massa.net>

///! This file provides structures representing changes to the asynchronous message pool
use crate::message::{AsyncMessage, AsyncMessageId};

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
    pub u64_serializer: U64VarIntSerializer,
}

impl AsyncPoolChangesSerializer {
    fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Serializer<AsyncPoolChanges> for AsyncPoolChangesSerializer {
    fn serialize(
        &self,
        value: &AsyncPoolChanges,
    ) -> Result<Vec<u8>, massa_serialization::SerializeError> {
        let mut res = Vec::new();
        res.extend(self.u64_serializer.serialize(&(value.0.len() as u64))?);
        for change in &value.0 {
            match change {
                Change::Add(id, message) => res.extend(id),
                Change::Delete(id) => {}
            }
        }
        Ok(res)
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
