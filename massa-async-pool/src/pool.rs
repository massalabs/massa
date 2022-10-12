//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a finite size final pool of asynchronous messages for use in the context of autonomous smart contracts

use crate::{
    changes::{AsyncPoolChanges, Change},
    config::AsyncPoolConfig,
    message::{AsyncMessage, AsyncMessageId},
};
use massa_models::{error::ModelsError, slot::Slot, streaming_step::StreamingStep};
use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};

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

    /// Applies pre-compiled `AsyncPoolChanges` to the pool without checking for overflows.
    /// This function is used when applying pre-compiled `AsyncPoolChanges` to an `AsyncPool`.
    ///
    /// # arguments
    /// * `changes`: `AsyncPoolChanges` listing all asynchronous pool changes (message insertions/deletions)
    pub fn apply_changes_unchecked(&mut self, changes: &AsyncPoolChanges) {
        for change in changes.0.iter() {
            match change {
                // add a new message to the pool
                Change::Add(msg_id, msg) => {
                    self.messages.insert(*msg_id, msg.clone());
                }

                // delete a message from the pool
                Change::Delete(msg_id) => {
                    self.messages.remove(msg_id);
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
        slot: &Slot,
        new_messages: &mut Vec<(AsyncMessageId, AsyncMessage)>,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        // Filter out all messages for which the validity end is expired.
        // Note that the validity_end bound is NOT included in the validity interval of the message.
        let mut eliminated: Vec<_> = self
            .messages
            .drain_filter(|_k, v| *slot >= v.validity_end)
            .chain(new_messages.drain_filter(|(_k, v)| *slot >= v.validity_end))
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

    /// Get a part of the async pool.
    /// Used for bootstrap.
    ///
    /// # Arguments
    /// * cursor: current bootstrap state
    ///
    /// # Returns
    /// The async pool part and the updated cursor
    pub fn get_pool_part(
        &self,
        cursor: StreamingStep<AsyncMessageId>,
    ) -> Result<
        (
            BTreeMap<AsyncMessageId, AsyncMessage>,
            StreamingStep<AsyncMessageId>,
        ),
        ModelsError,
    > {
        let left_bound = match cursor {
            StreamingStep::Started => Unbounded,
            StreamingStep::Ongoing(last_id) => {
                if last_id
                    == self
                        .messages
                        .first_key_value()
                        .map(|(&message_id, _)| message_id)
                        .expect("async_pool should contain at least one message")
                {
                    return Ok((BTreeMap::new(), StreamingStep::Finished(Some(last_id))));
                }
                Excluded(last_id)
            }
            StreamingStep::Finished(_) => return Ok((BTreeMap::new(), cursor)),
        };
        let mut pool_part = BTreeMap::new();
        for (id, message) in self.messages.range((left_bound, Unbounded)) {
            if pool_part.len() < self.config.part_size_message_bytes as usize {
                pool_part.insert(id.clone(), message.clone());
            }
        }
        let part_last_id = pool_part
            .last_key_value()
            .map(|(&message_id, _)| message_id)
            .expect("pool_part should contain at least one message");
        Ok((pool_part, StreamingStep::Ongoing(part_last_id)))
    }

    /// Set a part of the async pool.
    /// Used for bootstrap.
    ///
    /// # Arguments
    /// * part: the async pool part provided by `get_pool_part`
    ///
    /// # Returns
    /// The updated cursor after the current insert
    pub fn set_pool_part<'a>(
        &mut self,
        part: BTreeMap<AsyncMessageId, AsyncMessage>,
    ) -> Result<StreamingStep<AsyncMessageId>, ModelsError> {
        self.messages.extend(part);
        Ok(StreamingStep::Ongoing(
            self.messages
                .last_key_value()
                .map(|(&id, _)| id)
                .expect("should contain at least one message"),
        ))
    }
}

#[test]
fn test_take_batch() {
    use massa_hash::Hash;
    use massa_models::{address::Address, amount::Amount, slot::Slot};
    use std::str::FromStr;

    let config = AsyncPoolConfig {
        thread_count: 2,
        max_length: 10,
        max_data_async_message: 1000000,
        part_size_message_bytes: 1_000_000,
    };
    let mut pool = AsyncPool::new(config);
    let address = Address(Hash::compute_from(b"abc"));
    for i in 1..10 {
        pool.messages.insert(
            (
                std::cmp::Reverse(Amount::from_mantissa_scale(i, 0)),
                Slot::new(0, 0),
                0,
            ),
            AsyncMessage {
                emission_slot: Slot::new(0, 0),
                emission_index: 0,
                sender: address,
                destination: address,
                handler: "function".to_string(),
                validity_start: Slot::new(1, 0),
                validity_end: Slot::new(3, 0),
                max_gas: i,
                gas_price: Amount::from_str("0.1").unwrap(),
                coins: Amount::from_str("0.3").unwrap(),
                data: Vec::new(),
            },
        );
    }
    assert_eq!(pool.messages.len(), 9);
    pool.take_batch_to_execute(Slot::new(2, 0), 19);
    assert_eq!(pool.messages.len(), 6);
}
