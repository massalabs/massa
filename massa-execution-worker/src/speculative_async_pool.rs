// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative asynchronous pool represents the state of
//! the pool at an arbitrary execution slot.

use crate::active_history::{ActiveHistory, HistorySearchResult::Present};
use massa_async_pool::{
    AsyncMessage, AsyncMessageId, AsyncMessageInfo, AsyncMessageTrigger, AsyncMessageUpdate,
    AsyncPoolChanges,
};
use massa_final_state::FinalState;
use massa_ledger_exports::{Applicable, LedgerChanges, SetUpdateOrDelete};
use massa_models::slot::Slot;
use parking_lot::RwLock;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

pub(crate) struct SpeculativeAsyncPool {
    final_state: Arc<RwLock<FinalState>>,
    active_history: Arc<RwLock<ActiveHistory>>,
    // current speculative pool changes
    pool_changes: AsyncPoolChanges,
    // Used to know which messages we want to take
    message_infos: BTreeMap<AsyncMessageId, AsyncMessageInfo>,
}

impl SpeculativeAsyncPool {
    /// Creates a new `SpeculativeAsyncPool`
    ///
    /// # Arguments
    pub fn new(
        final_state: Arc<RwLock<FinalState>>,
        active_history: Arc<RwLock<ActiveHistory>>,
    ) -> Self {
        let mut message_infos = final_state.read().async_pool.message_info_cache.clone();

        for history_item in active_history.read().0.iter() {
            for change in history_item.state_changes.async_pool_changes.0.iter() {
                match change {
                    (id, SetUpdateOrDelete::Set(message)) => {
                        message_infos.insert(*id, AsyncMessageInfo::from(message.clone()));
                    }

                    (id, SetUpdateOrDelete::Update(message_update)) => {
                        message_infos.entry(*id).and_modify(|message_info| {
                            message_info.apply(message_update.clone());
                        });
                    }

                    (id, SetUpdateOrDelete::Delete) => {
                        message_infos.remove(id);
                    }
                }
            }
        }

        SpeculativeAsyncPool {
            final_state,
            active_history,
            pool_changes: Default::default(),
            message_infos,
        }
    }

    /// Returns the changes caused to the `SpeculativeAsyncPool` since its creation,
    /// and resets their local value to nothing.
    /// This must be called after `settle_emitted_messages()`
    pub fn take(&mut self) -> AsyncPoolChanges {
        std::mem::take(&mut self.pool_changes)
    }

    /// Takes a snapshot (clone) of the emitted messages
    pub fn get_snapshot(&self) -> AsyncPoolChanges {
        self.pool_changes.clone()
    }

    /// Resets the `SpeculativeAsyncPool` emitted messages to a snapshot (see `get_snapshot` method)
    pub fn reset_to_snapshot(&mut self, snapshot: AsyncPoolChanges) {
        self.pool_changes = snapshot;
    }

    /// Add a new message to the list of changes of this `SpeculativeAsyncPool`
    pub fn push_new_message(&mut self, msg: AsyncMessage) {
        self.pool_changes.push_add(msg.compute_id(), msg.clone());
        self.message_infos.insert(msg.compute_id(), msg.into());
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
            .extract_if(|_k, v| *slot >= v.validity_end)
            .collect();

        let eliminated_new_messages: Vec<_> = self
            .pool_changes
            .0
            .extract_if(|_k, v| match v {
                SetUpdateOrDelete::Set(v) => *slot >= v.validity_end,
                SetUpdateOrDelete::Update(_v) => false,
                SetUpdateOrDelete::Delete => false,
            })
            .collect();

        eliminated_infos.extend(eliminated_new_messages.iter().filter_map(|(k, v)| match v {
            SetUpdateOrDelete::Set(v) => Some((*k, AsyncMessageInfo::from(v.clone()))),
            SetUpdateOrDelete::Update(_v) => None,
            SetUpdateOrDelete::Delete => None,
        }));

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
            if let Some(filter) = &message_info.trigger /*&& !message_info.can_be_executed*/ && is_triggered(filter, ledger_changes)
            {
                message_info.can_be_executed = true;
                triggered_info.push((*id, message_info.clone()));
            }
        }

        // Query triggered messages
        let triggered_msg =
            self.fetch_msgs(triggered_info.iter().map(|(id, _)| id).collect(), false);

        for (msg_id, _msg) in triggered_msg.iter() {
            self.pool_changes.push_activate(*msg_id);
        }

        // Query eliminated messages
        let eliminated_msg =
            self.fetch_msgs(eliminated_infos.iter().map(|(id, _)| id).collect(), true);

        eliminated_msg
    }

    fn fetch_msgs(
        &mut self,
        mut wanted_ids: Vec<&AsyncMessageId>,
        delete_existing: bool,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        let mut msgs = Vec::new();

        let mut current_changes = HashMap::new();
        for id in wanted_ids.iter() {
            current_changes.insert(*id, AsyncMessageUpdate::default());
        }

        let pool_changes_clone = self.pool_changes.clone();

        // First, look in speculative pool
        wanted_ids.retain(|&message_id| match pool_changes_clone.0.get(message_id) {
            Some(SetUpdateOrDelete::Set(msg)) => {
                if delete_existing {
                    self.pool_changes.push_delete(*message_id);
                }
                msgs.push((*message_id, msg.clone()));
                false
            }
            Some(SetUpdateOrDelete::Update(msg_update)) => {
                current_changes.entry(message_id).and_modify(|e| {
                    e.apply(msg_update.clone());
                });
                true
            }
            Some(SetUpdateOrDelete::Delete) => true,
            None => true,
        });

        // Then, search the active history
        wanted_ids.retain(|&message_id| {
            match self.active_history.read().fetch_message(
                message_id,
                current_changes.get(message_id).cloned().unwrap_or_default(),
            ) {
                Present(SetUpdateOrDelete::Set(mut msg)) => {
                    msg.apply(current_changes.get(message_id).cloned().unwrap_or_default());
                    if delete_existing {
                        self.pool_changes.push_delete(*message_id);
                    }
                    msgs.push((*message_id, msg));
                    return false;
                }
                Present(SetUpdateOrDelete::Update(msg_update)) => {
                    current_changes.entry(message_id).and_modify(|e| {
                        e.apply(msg_update.clone());
                    });
                    return true;
                }
                _ => {}
            }
            true
        });

        // Then, fetch all the remaining messages from the final state
        let fetched_msgs = self
            .final_state
            .read()
            .async_pool
            .fetch_messages(wanted_ids);

        for (message_id, message) in fetched_msgs {
            if let Some(msg) = message {
                let mut msg = msg.clone();
                msg.apply(current_changes.get(message_id).cloned().unwrap_or_default());
                if delete_existing {
                    self.pool_changes.push_delete(*message_id);
                }
                msgs.push((*message_id, msg));
            }
        }

        msgs
    }

    #[cfg(any(test, feature = "test"))]
    pub fn get_message_infos(&self) -> BTreeMap<AsyncMessageId, AsyncMessageInfo> {
        self.message_infos.clone()
    }
}

/// Check in the ledger changes if a message trigger has been triggered
fn is_triggered(filter: &AsyncMessageTrigger, ledger_changes: &LedgerChanges) -> bool {
    ledger_changes.has_changes(&filter.address, filter.datastore_key.clone())
}
