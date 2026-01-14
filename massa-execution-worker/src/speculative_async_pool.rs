// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative asynchronous pool represents the state of
//! the pool at an arbitrary execution slot.

use crate::active_history::ActiveHistory;
use massa_async_pool::AsyncPoolChanges;
use massa_final_state::FinalStateController;
use massa_ledger_exports::LedgerChanges;
use massa_models::async_msg::{AsyncMessage, AsyncMessageTrigger};
use massa_models::async_msg_id::AsyncMessageId;
use massa_models::slot::Slot;
use massa_models::types::{Applicable, SetUpdateOrDelete};
use parking_lot::RwLock;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) struct SpeculativeAsyncPool {
    /// Async pool max length
    async_pool_max_length: u64,
    // current speculative pool changes
    pool_changes: AsyncPoolChanges,
    // Local cache of async messages
    message_cache: BTreeMap<AsyncMessageId, AsyncMessage>,
}

impl SpeculativeAsyncPool {
    /// Creates a new `SpeculativeAsyncPool`
    ///
    /// # Arguments
    pub fn new(
        final_state: Arc<RwLock<dyn FinalStateController>>,
        active_history: Arc<RwLock<ActiveHistory>>,
    ) -> Self {
        // fetch final state
        let async_pool_max_length;
        let mut message_cache;
        {
            let final_state_lock = final_state.read();
            let async_pool = final_state_lock.get_async_pool();
            async_pool_max_length = async_pool.config.max_length;
            message_cache = async_pool.message_cache.clone();
        }

        // apply history
        for history_item in active_history.read().0.iter() {
            for change in history_item.state_changes.async_pool_changes.0.iter() {
                match change {
                    (id, SetUpdateOrDelete::Set(message)) => {
                        message_cache.insert(*id, message.clone());
                    }

                    (id, SetUpdateOrDelete::Update(message_update)) => {
                        message_cache.entry(*id).and_modify(|message| {
                            message.apply(message_update.clone());
                        });
                    }

                    (id, SetUpdateOrDelete::Delete) => {
                        message_cache.remove(id);
                    }
                }
            }
        }

        SpeculativeAsyncPool {
            async_pool_max_length,
            pool_changes: Default::default(),
            message_cache,
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
    pub fn get_snapshot(&self) -> (AsyncPoolChanges, BTreeMap<AsyncMessageId, AsyncMessage>) {
        (self.pool_changes.clone(), self.message_cache.clone())
    }

    /// Resets the `SpeculativeAsyncPool` emitted messages to a snapshot (see `get_snapshot` method)
    pub fn reset_to_snapshot(
        &mut self,
        snapshot: (AsyncPoolChanges, BTreeMap<AsyncMessageId, AsyncMessage>),
    ) {
        self.pool_changes = snapshot.0;
        self.message_cache = snapshot.1;
    }

    /// Add a new message to the list of changes of this `SpeculativeAsyncPool`
    pub fn push_new_message(&mut self, msg: AsyncMessage) {
        self.pool_changes.push_add(msg.compute_id(), msg.clone());
        self.message_cache.insert(msg.compute_id(), msg);
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

        // Choose which messages to take based on the message_cache
        // (all messages are considered: finals, in active_history and in speculative)

        let mut wanted_ids = Vec::new();
        for (message_id, message) in self.message_cache.iter() {
            let corrected_max_gas = message.max_gas.saturating_add(async_msg_cst_gas_cost);
            // Note: SecureShareOperation.get_validity_range(...) returns RangeInclusive
            //       so to be consistent here, use >= & <= checks
            if available_gas >= corrected_max_gas
                && Self::is_message_ready_to_execute(
                    &slot,
                    &message.validity_start,
                    &message.validity_end,
                )
                && message.can_be_executed
            {
                available_gas -= corrected_max_gas;

                wanted_ids.push(*message_id);
            }
        }

        // Remove the messages_info of the taken messages, and push their deletion in the pool changes
        let mut taken_msgs = Vec::with_capacity(wanted_ids.len());
        for msg_id in &wanted_ids {
            taken_msgs.push((
                *msg_id,
                self.message_cache.remove(msg_id).unwrap(), // won't panic, items were listed above
            ));
        }
        self.delete_messages(wanted_ids);

        println!("LEO - slot {}: take_batch_to_execute: {:?}", slot, taken_msgs);

        taken_msgs
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
        // Update eliminated_msgs: remove messages that should be removed
        // Filter out all messages for which the validity end is expired.
        // Note: that the validity_end bound is included in the validity interval of the message.

        let mut eliminated_msgs = Vec::new();

        self.message_cache.retain(|id, msg| {
            if Self::is_message_expired(slot, &msg.validity_end) {
                eliminated_msgs.push((*id, msg.clone()));
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

        eliminated_msgs.extend(eliminated_new_messages.iter().filter_map(|(k, v)| match v {
            SetUpdateOrDelete::Set(v) => Some((*k, v.clone())),
            SetUpdateOrDelete::Update(_v) => None,
            SetUpdateOrDelete::Delete => None,
        }));

        // Truncate message pool to its max size, removing non-priority items
        let excess_count = self
            .message_cache
            .len()
            .saturating_sub(self.async_pool_max_length as usize);

        eliminated_msgs.reserve_exact(excess_count);
        for _ in 0..excess_count {
            eliminated_msgs.push(self.message_cache.pop_last().unwrap()); // will not panic (checked at excess_count computation)
        }

        // Activate the messages that can be activated (triggered)
        for (id, msg) in self.message_cache.iter_mut() {
            if let Some(filter) = &msg.trigger {
                if is_triggered(filter, ledger_changes) {
                    msg.can_be_executed = true;
                    self.pool_changes.push_activate(*id);
                }
            }
        }

        // Push message deletion to the pool changes
        self.delete_messages(eliminated_msgs.iter().map(|(id, _)| *id).collect());

        // reintroduce newly eliminated messages
        eliminated_msgs.extend(eliminated_new_messages.iter().filter_map(|(k, v)| match v {
            SetUpdateOrDelete::Set(v) => Some((*k, v.clone())),
            SetUpdateOrDelete::Update(_v) => None,
            SetUpdateOrDelete::Delete => None,
        }));

        println!("LEO - slot {}: settle slot eliminated_msgs: {:?}", slot, eliminated_msgs);

        eliminated_msgs
    }

    fn delete_messages(&mut self, message_ids: Vec<AsyncMessageId>) {
        for message_id in message_ids {
            self.pool_changes.push_delete(message_id);
        }
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
    ledger_changes.has_writes(&filter.address, filter.datastore_key.clone())
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
            &slot_validity_end,
        ));
        assert!(SpeculativeAsyncPool::is_message_ready_to_execute(
            &slot1,
            &slot_validity_start,
            &slot_validity_end,
        ));

        assert!(!SpeculativeAsyncPool::is_message_expired(
            &slot_validity_start,
            &slot_validity_end,
        ));
        assert!(SpeculativeAsyncPool::is_message_ready_to_execute(
            &slot_validity_start,
            &slot_validity_start,
            &slot_validity_end,
        ));

        assert!(!SpeculativeAsyncPool::is_message_expired(
            &slot_validity_end,
            &slot_validity_end,
        ));
        assert!(SpeculativeAsyncPool::is_message_ready_to_execute(
            &slot_validity_end,
            &slot_validity_start,
            &slot_validity_end,
        ));

        assert!(SpeculativeAsyncPool::is_message_expired(
            &slot2,
            &slot_validity_end,
        ));
        assert!(!SpeculativeAsyncPool::is_message_ready_to_execute(
            &slot2,
            &slot_validity_start,
            &slot_validity_end,
        ));
    }
}
