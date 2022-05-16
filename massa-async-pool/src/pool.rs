//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a finite size final pool of asynchronous messages for use in the context of autonomous smart contracts

use crate::{
    bootstrap::AsyncPoolBootstrap,
    changes::{AsyncPoolChanges, Change},
    config::AsyncPoolConfig,
    message::{AsyncMessage, AsyncMessageId},
};
use massa_models::Slot;
use std::collections::BTreeMap;

/// Represents a pool of sorted messages in a deterministic way.
/// The final asynchronous pool is attached to the output of the latest final slot within the context of massa-final-state.
/// Nodes must bootstrap the final message pool when they join the network.
#[derive(Debug, Clone)]
pub struct AsyncPool {
    /// Asynchronous pool configuration
    config: AsyncPoolConfig,

    /// Messages sorted by decreasing ID (decreasing priority)
    pub(crate) messages: BTreeMap<AsyncMessageId, AsyncMessage>,
}

impl AsyncPool {
    /// Creates an empty `AsyncPool`
    pub fn new(config: AsyncPoolConfig) -> AsyncPool {
        AsyncPool {
            config,
            messages: Default::default(),
        }
    }

    /// Creates an `AsyncPool` from a bootstrap snapshot obtained using `AsyncPool::get_bootstrap_snapshot`
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

    /// Returns a snapshot clone of the `AsyncPool` for bootstrapping other nodes
    pub fn get_bootstrap_snapshot(&self) -> AsyncPoolBootstrap {
        AsyncPoolBootstrap {
            messages: self.messages.values().cloned().collect(),
        }
    }

    /// Applies pre-compiled `AsyncPoolChanges` to the pool without checking for overflows.
    /// This function is used when applying pre-compiled `AsyncPoolChanges` to an `AsyncPool`.
    ///
    /// # arguments
    /// * `changes`: `AsyncPoolChanges` listing all asynchronous pool changes (message insertions/deletions)
    pub fn apply_changes_unchecked(&mut self, changes: AsyncPoolChanges) {
        for change in changes.0.into_iter() {
            match change {
                // add a new message to the pool
                Change::Add(msg_id, msg) => {
                    self.messages.insert(msg_id, msg);
                }

                // delete a message from the pool
                Change::Delete(msg_id) => {
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
    /// * `slot`: used to filter out expired messages, not stored
    /// * `new_messages`: list of `AsyncMessage` to add to the pool
    ///
    /// # returns
    /// The list of `(message_id, message)` that were eliminated from the pool after the changes were applied, sorted in the following order:
    /// * expired messages from the pool, in priority order (from highest to lowest priority)
    /// * expired messages from `new_messages` (in the order they appear in `new_messages`)
    /// * excess messages after inserting all remaining `new_messages`, in priority order (from highest to lowest priority)
    pub fn settle_slot(
        &mut self,
        slot: Slot,
        new_messages: &mut Vec<(AsyncMessageId, AsyncMessage)>,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        // Filter out all messages for which the validity end is expired.
        // Note that the validity_end bound is NOT included in the validity interval of the message.
        let mut eliminated: Vec<_> = self
            .messages
            .drain_filter(|_k, v| slot >= v.validity_end)
            .chain(new_messages.drain_filter(|(_k, v)| slot >= v.validity_end))
            .collect();

        // Insert new messages into the pool
        self.messages.extend(new_messages.clone());

        // Truncate message pool to its max size, removing non-prioritary items
        let excess_count = self
            .messages
            .len()
            .saturating_sub(self.config.max_length as usize);
        eliminated.reserve_exact(excess_count);
        for _ in 0..excess_count {
            eliminated.push(self.messages.pop_last().unwrap()); // will not panic (checked at excess_count computation)
        }
        eliminated
    }

    /// Takes the best possible batch of messages to execute, with gas limits and slot validity filtering.
    /// The returned messages are removed from the pool.
    /// This method is used at the beginning of a slot execution to list asynchronous messages to execute.
    ///
    /// # arguments
    /// * `slot`: select only messages that are valid within this slot
    /// * `available_gas`: maximum amount of available gas
    ///
    /// # returns
    /// A vector of messages, sorted from the most priority to the least priority
    pub fn take_batch_to_execute(
        &mut self,
        slot: Slot,
        mut available_gas: u64,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        // gather all selected items and remove them from self.messages
        // iterate in decreasing priority order
        self.messages
            .drain_filter(|_, msg| {
                // check available gas and validity period
                if available_gas >= msg.max_gas
                    && slot >= msg.validity_start
                    && slot < msg.validity_end
                {
                    available_gas -= msg.max_gas;
                    true
                } else {
                    false
                }
            })
            .collect()
    }
}

#[test]
fn test_take_batch() {
    use massa_hash::Hash;
    use massa_models::{Address, Amount, Slot};

    let config = AsyncPoolConfig { max_length: 10 };
    let mut pool = AsyncPool::new(config);
    let address = Address(Hash::compute_from(b"abc"));
    for i in 1..10 {
        pool.messages.insert(
            (std::cmp::Reverse(Amount::from_raw(i)), Slot::new(0, 0), 0),
            AsyncMessage {
                emission_slot: Slot::new(0, 0),
                emission_index: 0,
                sender: address,
                destination: address,
                handler: "function".to_string(),
                validity_start: Slot::new(1, 0),
                validity_end: Slot::new(3, 0),
                max_gas: i,
                gas_price: Amount::from_raw(1),
                coins: Amount::from_raw(0),
                data: Vec::new(),
            },
        );
    }
    assert_eq!(pool.messages.len(), 9);
    pool.take_batch_to_execute(Slot::new(2, 0), 19);
    assert_eq!(pool.messages.len(), 6);
}
