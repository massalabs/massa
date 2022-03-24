//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a finite size final pool of async messages for use in the context of autonomous smart contracts

use std::collections::BTreeMap;

use massa_models::Slot;

use crate::{
    bootstrap::AsyncPoolBootstrap,
    changes::{AddOrDelete, AsyncPoolChanges},
    config::AsyncPoolConfig,
    message::{AsyncMessage, AsyncMessageId},
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

    /// Creates an AsyncPool from a bootstrap snapshot obtained using AsyncPool::get_bootstrap_snapshot
    pub fn from_bootstrap_snapshot(
        config: AsyncPoolConfig,
        snapshot: AsyncPoolBootstrap,
    ) -> AsyncPool {
        AsyncPool {
            config,
            messages: snapshot
                .messages
                .into_iter()
                .map(|msg| (msg.compute_id(), msg))
                .collect(),
        }
    }

    /// Returns a snapshot clone of the AsyncPool for bootstrapping other nodes
    pub fn get_bootstrap_snapshot(&self) -> AsyncPoolBootstrap {
        AsyncPoolBootstrap {
            messages: self.messages.values().cloned().collect(),
        }
    }

    /// Applies precompiled AsyncPoolChanges to the pool without checking for overflows.
    /// This function is used when applying pre-compiled AsyncPoolChanges to an AsyncPool.
    ///
    /// # arguments
    /// * changes: AsyncPoolChanges listing all async pool changes (message insertions/deletions)
    pub fn apply_changes_unchecked(&mut self, changes: AsyncPoolChanges) {
        for change in changes.0 {
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
    /// This method is called at the end of a slot execution to apply the list of emitted messages,
    /// and get the list of pruned messages for `coins` reimbursement.
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
        mut new_messages: Vec<(AsyncMessageId, AsyncMessage)>,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
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
        eliminated.reserve_exact(excess_count);
        for _ in 0..excess_count {
            eliminated.push(self.messages.pop_first().unwrap()); // will not panic (checked at excess_count computation)
        }

        eliminated
    }

    /// Takes the best possible batch of messages to execute, with gas limits and slot validity filtering.
    /// The returned messages are removed from the pool.
    /// This method is used at the beginning of a slot execution to list async messages to execute.
    ///
    /// # arguments
    /// * slot: select only messages that are valid within this slot
    /// * available_gas: maximum amount of available gas
    ///
    /// # returns
    /// A vector of messages, sorted from the most prioritary to the least prioritary
    pub fn take_batch_to_executte(
        &mut self,
        slot: Slot,
        mut available_gas: u64,
    ) -> Vec<AsyncMessage> {
        let mut selected = Vec::new();

        // iterate in decreasing priority order
        for (msg_id, msg) in self.messages.iter().rev() {
            tracing::warn!("TAKE BATCH, AVAILABLE GAS = {}", msg.max_gas);
            // check validity period
            if slot < msg.validity_start || slot >= msg.validity_end {
                continue;
            }
            tracing::warn!("PASSED VALIDITY CHECK");

            // check available gas
            if available_gas < msg.max_gas {
                continue;
            }
            tracing::warn!("GOOD GAS CHECK");

            // add to selected items
            selected.push(*msg_id);

            // substract available gas
            available_gas -= msg.max_gas;

            // if there is no more gas, quit
            if available_gas == 0 {
                break;
            }
        }

        // gather all selected items and remove them from self.messages
        let mut accumulator = Vec::with_capacity(selected.len());
        for delete_id in selected {
            if let Some(v) = self.messages.remove(&delete_id) {
                accumulator.push(v);
            }
        }
        accumulator
    }
}
