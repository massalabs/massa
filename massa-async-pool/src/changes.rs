//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file provides structures representing changes to the async message pool

use crate::message::{AsyncMessage, AsyncMessageId};

/// Enum representing a add/delete change on a value T
#[derive(Debug, Clone)]
pub enum AddOrDelete<T> {
    /// adds a new value T
    Add(T),

    /// deletes the value T
    Delete,
}

/// allows applying another AddOrDelete to the current one
impl<T> AddOrDelete<T> {
    fn apply(&mut self, other: Self) {
        *self = other;
    }
}

/// represents a list of additions and deletions to the async message pool
#[derive(Default, Debug, Clone)]
pub struct AsyncPoolChanges(pub(crate) Vec<(AsyncMessageId, AddOrDelete<AsyncMessage>)>);

impl AsyncPoolChanges {
    /// Extends self with another another AsyncPoolChanges.
    /// This simply appends the contents of other to self.
    /// No add/delete compensations are done.
    pub fn extend(&mut self, other: AsyncPoolChanges) {
        self.0.extend(other.0);
    }

    /// Pushes a message addition to the list of changes.
    /// No add/delete compensations are done.
    ///
    /// Arguments:
    /// * msg_id: ID of the message to push as added to the list of changes
    /// * msg: message to push as added to the list of changes
    pub fn push_add(&mut self, msg_id: AsyncMessageId, msg: AsyncMessage) {
        self.0.push((msg_id, AddOrDelete::Add(msg)));
    }

    /// Pushes a message deletion to the list of changes.
    /// No add/delete compensations are done.
    ///
    /// Arguments:
    /// * msg_id: ID of the message to push as deleted to the list of changes
    pub fn push_delete(&mut self, msg_id: AsyncMessageId) {
        self.0.push((msg_id, AddOrDelete::Delete));
    }

    /// Retrieves only the added messages
    pub fn get_add(&self) -> Vec<(AsyncMessageId, AsyncMessage)> {
        self.0
            .clone()
            .into_iter()
            .filter_map(|(id, v)| match v {
                AddOrDelete::Add(x) => Some((id, x)),
                AddOrDelete::Delete => None,
            })
            .collect::<Vec<(AsyncMessageId, AsyncMessage)>>()
    }
}
