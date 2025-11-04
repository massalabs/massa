// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative asynchronous pool represents the state of
//! the pool at an arbitrary execution slot.

use crate::active_history::ActiveHistory;
use massa_async_pool::AsyncPoolChanges;
use massa_final_state::FinalStateController;
use massa_ledger_exports::LedgerChanges;
use massa_models::async_msg::{AsyncMessage, AsyncMessageInfo, AsyncMessageTrigger};
use massa_models::async_msg_id::AsyncMessageId;
use massa_models::slot::Slot;
use massa_models::types::{Applicable, SetUpdateOrDelete};
use parking_lot::RwLock;
use std::{collections::BTreeMap, sync::Arc, time::Instant};

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

                wanted_messages.push(*message_id);
            }
        }

        let taken = self.fetch_msgs(wanted_messages);

        // Remove the messages_info of the taken messages, and push their deletion in the pool changes
        let taken_ids: Vec<_> = taken.iter().map(|(id, _)| *id).collect();
        for message_id in taken_ids.iter() {
            self.message_infos.remove(message_id);
        }
        self.delete_messages(taken_ids);

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
        fix_eliminated_msg: bool,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        // Update the messages_info: remove messages that should be removed
        // Filter out all messages for which the validity end is expired.
        // Note: that the validity_end bound is included in the validity interval of the message.

        let start = Instant::now();
        let init_time = Instant::now();

        let mut eliminated_infos = Vec::new();
        self.message_infos.retain(|id, info| {
            if Self::is_message_expired(slot, &info.validity_end) {
                eliminated_infos.push((*id, info.clone()));
                false
            } else {
                true
            }
        });
        let duration = start.elapsed();
        tracing::info!(
            "slot {:?} message_infos retain iteration: message_infos_size={}, duration={:?}",
            slot,
            self.message_infos.len(),
            duration
        );

        let start = Instant::now();
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
        let duration = start.elapsed();
        tracing::info!(
            "slot {:?} eliminated_new_messages retain iteration: eliminated_new_messages_size={}, duration={:?}",
            slot,
            eliminated_new_messages.len(),
            duration
        );

        let start = Instant::now();
        // Truncate message pool to its max size, removing non-priority items
        let excess_count = self
            .message_infos
            .len()
            .saturating_sub(self.final_state.read().get_async_pool().config.max_length as usize);

        eliminated_infos.reserve_exact(excess_count);
        for _ in 0..excess_count {
            eliminated_infos.push(self.message_infos.pop_last().unwrap()); // will not panic (checked at excess_count computation)
        }
        let duration = start.elapsed();
        tracing::info!(
            "slot {:?} eliminated_infos retain iteration: eliminated_infos_size={}, duration={:?}",
            slot,
            eliminated_infos.len(),
            duration
        );

        let start = Instant::now();
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
        let duration = start.elapsed();
        tracing::info!(
            "slot {:?} triggered_info : triggered_info_size={}, duration={:?}",
            slot,
            triggered_info.len(),
            duration
        );

        let start = Instant::now();
        // Query triggered messages
        let triggered_msg = self.fetch_msgs(triggered_info.into_iter().map(|(id, _)| id).collect());

        let duration = start.elapsed();
        tracing::info!(
            "slot {:?} triggered_msg fetch_msgs iteration: triggered_msg_size={}, duration={:?}",
            slot,
            triggered_msg.len(),
            duration
        );

        let start = Instant::now();
        for (msg_id, _msg) in triggered_msg.iter() {
            self.pool_changes.push_activate(*msg_id);
        }
        let duration = start.elapsed();
        tracing::info!(
            "slot {:?} triggered_msg push_activate iteration: triggered_msg_size={}, duration={:?}",
            slot,
            triggered_msg.len(),
            duration
        );

        let start = Instant::now();
        // Query eliminated messages
        let mut eliminated_msg =
            self.fetch_msgs(eliminated_infos.into_iter().map(|(id, _)| id).collect());
        let duration = start.elapsed();
        tracing::info!(
            "slot {:?} eliminated_msg fetch_msgs iteration: eliminated_msg_size={}, duration={:?}",
            slot,
            eliminated_msg.len(),
            duration
        );

        let start = Instant::now();
        // Push their deletion in the pool changes
        self.delete_messages(eliminated_msg.iter().map(|(id, _)| *id).collect());

        let duration = start.elapsed();
        tracing::info!(
            "slot {:?} eliminated_msg delete_messages iteration: eliminated_msg_size={}, duration={:?}",
            slot,
            eliminated_msg.len(),
            duration
        );

        if fix_eliminated_msg {
            eliminated_msg.extend(eliminated_new_messages.iter().filter_map(|(k, v)| match v {
                SetUpdateOrDelete::Set(v) => Some((*k, v.clone())),
                SetUpdateOrDelete::Update(_v) => None,
                SetUpdateOrDelete::Delete => None,
            }));
        }

        let duration = init_time.elapsed();
        tracing::info!("slot {:?} settle_slot iteration: duration={:?}", slot, duration);
        tracing::info!("slot {:?} settle_slot as_millis: duration={:?}", slot, duration.as_millis());
        if duration.as_millis() > 500 {
            tracing::warn!(
                "WARNING DURATION: settle_slot iteration: duration={:?}",
                duration
            );
        }
        eliminated_msg
    }

    /// This version changes two things:
    /// - We ensure the order of the messages is preserved when we fetch them (to avoid a non-deterministic behavior in the execution)
    /// - We simplify the code by working from the final state and applying changes of the active history and the current changes
    fn fetch_msgs_opt(
        &mut self,
        wanted_ids: Vec<AsyncMessageId>,
    ) -> Vec<(AsyncMessageId, Option<AsyncMessage>)> {
        // fetch final state
        let mut retrieved = self
            .final_state
            .read()
            .get_async_pool()
            .fetch_messages(&wanted_ids);

        // function to accumulate changes to the retrieved list
        let mut apply_changes = |changes: &AsyncPoolChanges| {
            for (id, msg_opt) in retrieved.iter_mut() {
                match changes.0.get(id) {
                    Some(SetUpdateOrDelete::Set(msg)) => {
                        *msg_opt = Some(msg.clone());
                    }
                    Some(SetUpdateOrDelete::Update(msg_update)) => {
                        if msg_opt.is_none() {
                            *msg_opt = Some(AsyncMessage::default());
                        }
                        msg_opt.as_mut().unwrap().apply(msg_update.clone()); // unwrap checked above
                    }
                    Some(SetUpdateOrDelete::Delete) => {
                        *msg_opt = None;
                    }
                    None => {}
                }
            }
        };

        // fetch active history
        let start = Instant::now();
        let active_history = self.active_history.read();
        let active_history_size = active_history.0.len();
        let mut total_async_changes = 0;
        for hist_item in active_history.0.iter() {
            total_async_changes += hist_item.state_changes.async_pool_changes.0.len();
            apply_changes(&hist_item.state_changes.async_pool_changes);
        }
        drop(active_history);
        let duration = start.elapsed();
        tracing::info!(
            "active_history apply_changes iteration: active_history_size={}, async_changes={}, duration={:?}",
            active_history_size,
            total_async_changes,
            duration
        );

        // fetch current changes
        apply_changes(&self.pool_changes);

        retrieved
    }

    fn fetch_msgs(
        &mut self,
        wanted_ids: Vec<AsyncMessageId>,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        self.fetch_msgs_opt(wanted_ids)
            .into_iter()
            .filter_map(|(id, msg_opt)| msg_opt.map(|msg| (id, msg)))
            .collect()
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
