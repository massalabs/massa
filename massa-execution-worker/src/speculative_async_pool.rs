// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative asynchronous pool represents the state of
//! the pool at an arbitrary execution slot.

use crate::active_history::{ActiveHistory, HistorySearchResult::Present};
use massa_async_pool::{AsyncMessage, AsyncMessageId, AsyncMessageTrigger, AsyncPoolChanges};
use massa_final_state::FinalState;
use massa_ledger_exports::LedgerChanges;
use massa_models::slot::Slot;
use parking_lot::RwLock;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) struct SpeculativeAsyncPool {
    final_state: Arc<RwLock<FinalState>>,
    active_history: Arc<RwLock<ActiveHistory>>,
    // newly emitted messages
    emitted: Vec<(AsyncMessageId, AsyncMessage)>,
    // current speculative pool changes
    pool_changes: AsyncPoolChanges,
    // Used to know which messages we want to take
    message_infos: BTreeMap<AsyncMessageId, AsyncMessageInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsyncMessageInfo {
    pub validity_start: Slot,
    pub validity_end: Slot,
    pub max_gas: u64,
    pub can_be_executed: bool,
    pub trigger: Option<AsyncMessageTrigger>,
}

impl From<AsyncMessage> for AsyncMessageInfo {
    fn from(value: AsyncMessage) -> Self {
        Self {
            validity_start: value.validity_start,
            validity_end: value.validity_end,
            max_gas: value.max_gas,
            can_be_executed: value.can_be_executed,
            trigger: value.trigger,
        }
    }
}

impl SpeculativeAsyncPool {
    /// Creates a new `SpeculativeAsyncPool`
    ///
    /// # Arguments
    pub fn new(
        final_state: Arc<RwLock<FinalState>>,
        active_history: Arc<RwLock<ActiveHistory>>,
    ) -> Self {
        SpeculativeAsyncPool {
            final_state,
            active_history,
            emitted: Default::default(),
            pool_changes: Default::default(),
            message_infos: Default::default(),
        }
    }

    /// Returns the changes caused to the `SpeculativeAsyncPool` since its creation,
    /// and resets their local value to nothing.
    /// This must be called after `settle_emitted_messages()`
    pub fn take(&mut self) -> AsyncPoolChanges {
        std::mem::take(&mut self.pool_changes)
    }

    /// Takes a snapshot (clone) of the emitted messages
    pub fn get_snapshot(&self) -> Vec<(AsyncMessageId, AsyncMessage)> {
        self.emitted.clone()
    }

    /// Resets the `SpeculativeAsyncPool` emitted messages to a snapshot (see `get_snapshot` method)
    pub fn reset_to_snapshot(&mut self, snapshot: Vec<(AsyncMessageId, AsyncMessage)>) {
        self.emitted = snapshot;
    }

    /// Add a new message to the list of changes of this `SpeculativeAsyncPool`
    pub fn push_new_message(&mut self, msg: AsyncMessage) {
        self.emitted.push((msg.compute_id(), msg));
    }

    /// Takes a batch of asynchronous messages to execute,
    /// removing them from the speculative asynchronous pool and settling their deletion from it in the changes accumulator.
    ///
    /// # Arguments
    /// * `slot`: slot at which the batch is taken (allows filtering by validity interval)
    /// * `max_gas`: maximum amount of gas available
    ///
    /// # Returns
    /// A vector of `AsyncMessage` to execute
    pub fn take_batch_to_execute(
        &mut self,
        slot: Slot,
        max_gas: u64,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        let mut available_gas = max_gas;

        // Choose which messages to take based on self.message_infos
        // (all messages are considered: finals, in active_history and in speculative)

        let mut wanted_messages = Vec::new();

        let message_infos = self.message_infos.clone();

        for (message_id, message_info) in message_infos.iter() {
            if available_gas >= message_info.max_gas
                && slot >= message_info.validity_start
                && slot < message_info.validity_end
                && message_info.can_be_executed
            {
                available_gas -= message_info.max_gas;

                wanted_messages.push(message_id);
            }
        }

        let taken = self.fetch_msgs(wanted_messages, true);

        for (message_id, _) in taken.iter() {
            self.message_infos.remove(message_id);
        }

        taken
    }

    /// Settle a slot.
    /// Consume newly emitted messages into `self.async_pool`, recording changes into `self.settled_changes`.
    ///
    /// # Arguments
    /// * slot: slot that is being settled
    /// * ledger_changes: ledger changes for that slot, used to see if we can activate some messages
    ///
    /// # Returns
    /// the list of deleted `(message_id, message)`, used for reimbursement
    pub fn settle_slot(
        &mut self,
        slot: &Slot,
        ledger_changes: &LedgerChanges,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        // Update the messages_info: remove messages that should be removed
        // Filter out all messages for which the validity end is expired.
        // Note that the validity_end bound is NOT included in the validity interval of the message.

        let mut eliminated_infos: Vec<_> = self
            .message_infos
            .drain_filter(|_k, v| *slot >= v.validity_end)
            .collect();

        let eliminated_new_messages = self.emitted.drain_filter(|(_k, v)| *slot >= v.validity_end);

        eliminated_infos
            .extend(eliminated_new_messages.map(|(k, v)| (k, AsyncMessageInfo::from(v))));

        // Truncate message pool to its max size, removing non-prioritary items
        let excess_count = self
            .message_infos
            .len()
            .saturating_sub(self.final_state.read().async_pool.config.max_length as usize);

        eliminated_infos.reserve_exact(excess_count);
        for _ in 0..excess_count {
            eliminated_infos.push(self.message_infos.pop_last().unwrap()); // will not panic (checked at excess_count computation)
        }

        // Activate the messages that can be activated (triggered)
        let mut triggered_info = Vec::new();
        for (id, message_info) in self.message_infos.iter_mut() {
            if let Some(filter) = &message_info.trigger && !message_info.can_be_executed && is_triggered(filter, ledger_changes)
            {
                message_info.can_be_executed = true;
                triggered_info.push((*id, message_info.clone()));
            }
        }

        for (msg_id, msg) in std::mem::take(&mut self.emitted) {
            self.pool_changes.push_add(msg_id, msg);
        }

        // Query triggered messages
        let triggered_msg =
            self.fetch_msgs(eliminated_infos.iter().map(|(id, _)| id).collect(), false);

        for (msg_id, msg) in triggered_msg.iter() {
            self.pool_changes.push_activate(*msg_id, msg.clone());
        }

        // Query eliminated messages
        let eliminated_msg =
            self.fetch_msgs(eliminated_infos.iter().map(|(id, _)| id).collect(), true);

        eliminated_msg

        /*
        let (deleted_messages, triggered_messages) =
        self.async_pool
            .settle_slot(slot, &mut self.emitted, ledger_changes);*/

        /*let mut eliminated: Vec<_> = self
            .messages
            .drain_filter(|_k, v| *slot >= v.validity_end)
            .chain(self.emitted.drain_filter(|(_k, v)| *slot >= v.validity_end))
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
        let mut triggered = Vec::new();
        for (id, message) in self.messages.iter_mut() {
            if let Some(filter) = &message.trigger && !message.can_be_executed && is_triggered(filter, ledger_changes)
            {
                message.can_be_executed = true;
                triggered.push((*id, message.clone()));
            }
        }

        for (msg_id, msg) in std::mem::take(&mut self.emitted) {
            self.pool_changes.push_add(msg_id, msg);
        }

        for (msg_id, _msg) in deleted_messages.iter() {
            self.pool_changes.push_delete(*msg_id);
        }
        for (msg_id, msg) in triggered_messages.iter() {
            self.pool_changes.push_activate(*msg_id, msg);
        }
        deleted_messages*/
    }

    fn fetch_msgs(
        &mut self,
        mut wanted_ids: Vec<&AsyncMessageId>,
        delete_existing: bool,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        let mut msgs = Vec::new();

        // First, look in speculative pool
        wanted_ids.drain_filter(|&mut message_id| {
            for (id, msg) in self.emitted.iter() {
                if id == message_id {
                    if delete_existing {
                        self.pool_changes.push_delete(*message_id);
                    }
                    msgs.push((*message_id, msg.clone()));
                    return true;
                }
            }
            false
        });

        // Then, search the active history
        wanted_ids.drain_filter(|&mut message_id| {
            if let Present(msg) = self.active_history.read().fetch_message(message_id) {
                if delete_existing {
                    self.pool_changes.push_delete(*message_id);
                }
                msgs.push((*message_id, msg));
                return true;
            }
            false
        });

        // Then, fetch all the remaining messages from the final state
        let fetched_msgs = self
            .final_state
            .read()
            .async_pool
            .fetch_messages(wanted_ids);

        for (message_id, message) in fetched_msgs {
            if let Some(msg) = message {
                if delete_existing {
                    self.pool_changes.push_delete(*message_id);
                }
                msgs.push((*message_id, msg));
            }
        }

        msgs
    }
}

/*

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
        let mut triggered = Vec::new();
        for (id, message) in self.messages.iter_mut() {
            if let Some(filter) = &message.trigger && !message.can_be_executed && is_triggered(filter, ledger_changes)
            {
                message.can_be_executed = true;
                triggered.push((*id, message.clone()));
            }
        }
        (eliminated, triggered)

 */

/// Check in the ledger changes if a message trigger has been triggered
fn is_triggered(filter: &AsyncMessageTrigger, ledger_changes: &LedgerChanges) -> bool {
    ledger_changes.has_changes(&filter.address, filter.datastore_key.clone())
}
