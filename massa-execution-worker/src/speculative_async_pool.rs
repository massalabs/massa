// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative asynchronous pool represents the state of
//! the pool at an arbitrary execution slot.

use crate::active_history::{ActiveHistory, HistorySearchResult::Present};
use massa_async_pool::{
    AsyncMessage, AsyncMessageId, AsyncMessageInfo, AsyncMessageTrigger, AsyncMessageUpdate,
    AsyncPoolChanges,
};
use massa_final_state::FinalStateController;
use massa_ledger_exports::{Applicable, LedgerChanges, SetUpdateOrDelete};
use massa_models::slot::Slot;
use parking_lot::RwLock;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

pub(crate) struct SpeculativeAsyncPool {
    final_state: Arc<RwLock<dyn FinalStateController>>,
    active_history: Arc<RwLock<ActiveHistory>>,
    // current speculative pool changes
    pool_changes: AsyncPoolChanges,
    // Used to know which messages we want to take (contains active and final messages)
    message_infos: BTreeMap<AsyncMessageId, AsyncMessageInfo>,
}

impl SpeculativeAsyncPool {
    /// Creates a new `SpeculativeAsyncPool`
    ///
    /// # Arguments
    pub fn new(
        final_state: Arc<RwLock<dyn FinalStateController>>,
        active_history: Arc<RwLock<ActiveHistory>>,
    ) -> Self {
        let mut message_infos = final_state
            .read()
            .get_async_pool()
            .message_info_cache
            .clone();

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
    /// The message_infos should already be removed if taken, no need to do it here.
    pub fn take(&mut self) -> AsyncPoolChanges {
        std::mem::take(&mut self.pool_changes)
    }

    /// Takes a snapshot (clone) of the emitted messages
    pub fn get_snapshot(&self) -> (AsyncPoolChanges, BTreeMap<AsyncMessageId, AsyncMessageInfo>) {
        (self.pool_changes.clone(), self.message_infos.clone())
    }

    /// Resets the `SpeculativeAsyncPool` emitted messages to a snapshot (see `get_snapshot` method)
    pub fn reset_to_snapshot(
        &mut self,
        snapshot: (AsyncPoolChanges, BTreeMap<AsyncMessageId, AsyncMessageInfo>),
    ) {
        self.pool_changes = snapshot.0;
        self.message_infos = snapshot.1;
    }

    /// Add a new message to the list of changes of this `SpeculativeAsyncPool`
    pub fn push_new_message(&mut self, msg: AsyncMessage) {
        self.pool_changes.push_add(msg.compute_id(), msg.clone());
        self.message_infos.insert(msg.compute_id(), msg.into());
    }

    /// Takes a batch of asynchronous messages to execute,
    /// removing them from the speculative asynchronous pool and settling their deletion from it
    /// in the changes accumulator.
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
        async_msg_cst_gas_cost: u64,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        let mut available_gas = max_gas;

        // Choose which messages to take based on self.message_infos
        // (all messages are considered: finals, in active_history and in speculative)

        let mut wanted_messages = Vec::new();

        let message_infos = self.message_infos.clone();

        for (message_id, message_info) in message_infos.iter() {
            let corrected_max_gas = message_info.max_gas.saturating_add(async_msg_cst_gas_cost);
            // Note: SecureShareOperation.get_validity_range(...) returns RangeInclusive
            //       so to be consistent here, use >= & <= checks
            if available_gas >= corrected_max_gas
                && Self::is_message_ready_to_execute(
                    &slot,
                    &message_info.validity_start,
                    &message_info.validity_end,
                )
                && message_info.can_be_executed
            {
                available_gas -= corrected_max_gas;

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
        // Note: that the validity_end bound is included in the validity interval of the message.

        let mut eliminated_infos = Vec::new();
        self.message_infos.retain(|id, info| {
            if Self::is_message_expired(slot, &info.validity_end) {
                eliminated_infos.push((*id, info.clone()));
                false
            } else {
                true
            }
        });

        let mut eliminated_new_messages = Vec::new();
        self.pool_changes.0.retain(|k, v| match v {
            SetUpdateOrDelete::Set(message) => {
                if Self::is_message_expired(slot, &message.validity_end) {
                    eliminated_new_messages.push((*k, v.clone()));
                    false
                } else {
                    true
                }
            }
            SetUpdateOrDelete::Update(_v) => true,
            SetUpdateOrDelete::Delete => true,
        });

        eliminated_infos.extend(eliminated_new_messages.iter().filter_map(|(k, v)| match v {
            SetUpdateOrDelete::Set(v) => Some((*k, AsyncMessageInfo::from(v.clone()))),
            SetUpdateOrDelete::Update(_v) => None,
            SetUpdateOrDelete::Delete => None,
        }));

        // Truncate message pool to its max size, removing non-priority items
        let excess_count = self
            .message_infos
            .len()
            .saturating_sub(self.final_state.read().get_async_pool().config.max_length as usize);

        eliminated_infos.reserve_exact(excess_count);
        for _ in 0..excess_count {
            eliminated_infos.push(self.message_infos.pop_last().unwrap()); // will not panic (checked at excess_count computation)
        }

        // Activate the messages that can be activated (triggered)
        let mut triggered_info = Vec::new();
        for (id, message_info) in self.message_infos.iter_mut() {
            if let Some(filter) = &message_info.trigger {
                if is_triggered(filter, ledger_changes) {
                    message_info.can_be_executed = true;
                    triggered_info.push((*id, message_info.clone()));
                }
            }
        }

        // Query triggered messages
        let triggered_msg =
            self.fetch_msgs(triggered_info.iter().map(|(id, _)| id).collect(), false);

        for (msg_id, _msg) in triggered_msg.iter() {
            self.pool_changes.push_activate(*msg_id);
        }

        // Query eliminated messages
        let mut eliminated_msg =
            self.fetch_msgs(eliminated_infos.iter().map(|(id, _)| id).collect(), true);

        eliminated_msg.extend(eliminated_new_messages.iter().filter_map(|(k, v)| match v {
            SetUpdateOrDelete::Set(v) => Some((*k, v.clone())),
            SetUpdateOrDelete::Update(_v) => None,
            SetUpdateOrDelete::Delete => None,
        }));
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
            .get_async_pool()
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

    /// Return true if a message (given its validity end) is expired
    /// Must be consistent with is_message_valid
    fn is_message_expired(slot: &Slot, message_validity_end: &Slot) -> bool {
        // Note: SecureShareOperation.get_validity_range(...) returns RangeInclusive
        //       (for operation validity) so apply the same rule for message validity
        *slot > *message_validity_end
    }

    /// Return true if a message (given its validity_start & validity end) is ready to execute
    /// Must be consistent with is_message_expired
    fn is_message_ready_to_execute(
        slot: &Slot,
        message_validity_start: &Slot,
        message_validity_end: &Slot,
    ) -> bool {
        // Note: SecureShareOperation.get_validity_range(...) returns RangeInclusive
        //       (for operation validity) so apply the same rule for message validity
        slot >= message_validity_start && slot <= message_validity_end
    }
}

/// Check in the ledger changes if a message trigger has been triggered
fn is_triggered(filter: &AsyncMessageTrigger, ledger_changes: &LedgerChanges) -> bool {
    ledger_changes.has_changes(&filter.address, filter.datastore_key.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test if is_message_expired & is_message_ready_to_execute are consistent
    #[test]
    fn test_validity() {
        let slot1 = Slot::new(6, 0);
        let slot2 = Slot::new(9, 0);
        let slot_validity_start = Slot::new(4, 0);
        let slot_validity_end = Slot::new(8, 0);

        assert!(!SpeculativeAsyncPool::is_message_expired(
            &slot1,
            &slot_validity_end
        ));
        assert!(SpeculativeAsyncPool::is_message_ready_to_execute(
            &slot1,
            &slot_validity_start,
            &slot_validity_end
        ));

        assert!(!SpeculativeAsyncPool::is_message_expired(
            &slot_validity_start,
            &slot_validity_end
        ));
        assert!(SpeculativeAsyncPool::is_message_ready_to_execute(
            &slot_validity_start,
            &slot_validity_start,
            &slot_validity_end
        ));

        assert!(!SpeculativeAsyncPool::is_message_expired(
            &slot_validity_end,
            &slot_validity_end
        ));
        assert!(SpeculativeAsyncPool::is_message_ready_to_execute(
            &slot_validity_end,
            &slot_validity_start,
            &slot_validity_end
        ));

        assert!(SpeculativeAsyncPool::is_message_expired(
            &slot2,
            &slot_validity_end
        ));
        assert!(!SpeculativeAsyncPool::is_message_ready_to_execute(
            &slot2,
            &slot_validity_start,
            &slot_validity_end
        ));
    }
}
