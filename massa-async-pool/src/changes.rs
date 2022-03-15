//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file provides structures representing changes to the async message pool

use crate::{
    message::{AsyncMessage, AsyncMessageId},
    types::AddOrDelete,
};

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
}
