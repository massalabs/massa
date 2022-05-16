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
pub struct AsyncPoolChanges(pub(crate) Vec<Change<AsyncMessageId, AsyncMessage>>);

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
