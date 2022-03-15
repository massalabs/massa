//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a finite size final pool of async messages for use in the context of autonomous smart contracts

use std::collections::BTreeMap;

use massa_models::Slot;

use crate::{
    changes::AsyncPoolChanges,
    config::AsyncPoolConfig,
    message::{AsyncMessage, AsyncMessageId},
    types::AddOrDelete,
};

/// Represents a pool of deterministically sorted messages.
/// The final async pool is attached to the output of the latest final slot within the context of massa-final-state.
/// Nodes must bootstrap the final message pool when they join the network.
#[derive(Debug, Clone)]
pub struct AsyncPool {
    /// async pool config
    config: AsyncPoolConfig,

    /// messages sorted by increasing ID (increasing priority)
    messages: BTreeMap<AsyncMessageId, AsyncMessage>,
}

impl AsyncPool {
    /// Creates an empty AsyncPool
    pub fn new(config: AsyncPoolConfig) -> AsyncPool {
        AsyncPool {
            config,
            messages: Default::default(),
        }
    }

    /// Applies precompiled AsyncPoolChanges to the pool without checking for overflows
    pub fn apply_changes_unchecked(&mut self, changes: AsyncPoolChanges) {
        for change in changes.0.into_iter() {
            match change {
                // add a new message to the pool
                (msg_id, AddOrDelete::Add(msg)) => {
                    self.messages.insert(msg_id, msg);
                }

                // delete a message from the pool
                (msg_id, AddOrDelete::Delete) => {
                    self.messages.remove(&msg_id);
                }
            }
        }
    }

    /// Settles a slot, adding new messages to the pool and returning expired and excess ones.
    ///
    /// # arguments
    /// * slot: used to filter out expired messages, not stored
    /// * new_messages: list of AsyncMessage to add to the pool
    ///
    /// # returns
    /// The list of (message_id, message) that were eliminated from the pool after the changes were applied, sorted in the following order:
    /// * expired messages from the pool, in priority order (from lowest to highest priority)
    /// * expired messages from new_messages (in the order they appear in new_messages)
    /// * excess messages after inserting all remaining new_messages, in priority order (from lowest to highest priority)
    pub fn settle_slot(
        &mut self,
        slot: Slot,
        new_messages: Vec<AsyncMessage>,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        // Compute IDs
        let mut new_messages: Vec<_> = new_messages
            .into_iter()
            .map(|v| (v.compute_id(), v))
            .collect();

        // Filter out all messages for which the validity end is expired.
        // Note that the validity_end bound is NOT included in the validity interval of the message.
        let mut eliminated: Vec<_> = self
            .messages
            .drain_filter(|_k, v| slot >= v.validity_end)
            .chain(new_messages.drain_filter(|(_k, v)| slot > v.validity_end))
            .collect();

        // Insert new messages into the pool
        self.messages.extend(new_messages);

        // Truncate message pool to its max size, removing non-prioritary items
        let excess_count = self
            .messages
            .len()
            .saturating_sub(self.config.max_length as usize);
        new_messages.reserve_exact(excess_count);
        for _ in 0..excess_count {
            eliminated.push(self.messages.pop_first().unwrap()); // will not panic (checked at excess_count computation)
        }

        eliminated
    }
}
