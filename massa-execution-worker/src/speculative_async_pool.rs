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

/// Execution component version (MIP-0002-BugFix) from which async-message batch selection
/// and pool eviction rank by `fee / (max_gas + async_msg_cst_gas_cost)` instead of
/// `fee / max_gas`, matching the gas actually charged per message in
/// [`SpeculativeAsyncPool::take_batch_to_execute`].
pub const ASYNC_MSG_EFFECTIVE_GAS_PRIORITY_EXEC_VERSION: u32 = 2;

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
    /// A message is only taken when `message.max_gas + async_msg_cst_gas_cost` still fits in
    /// the remaining budget. Messages asking for more than the largest budget a slot can ever
    /// get (`max_async_gas + max_gas_per_block`) are therefore never taken; `send_message`
    /// rejects those at emission from execution component version 2 on, but messages emitted
    /// before that activation can still be sitting in the pool, and they expire unexecuted.
    ///
    /// When `rank_by_effective_gas` is true (MIP-0002 / execution component version
    /// [`ASYNC_MSG_EFFECTIVE_GAS_PRIORITY_EXEC_VERSION`]), candidates are ordered by
    /// `fee / (max_gas + async_msg_cst_gas_cost)` so priority matches the budget charge.
    /// Otherwise ordering follows the stored [`AsyncMessageId`] (`fee / max_gas`).
    ///
    /// # Arguments
    /// * `slot`: slot at which the batch is taken (allows filtering by validity interval)
    /// * `max_gas`: maximum amount of gas available
    /// * `async_msg_cst_gas_cost`: fixed per-message gas surcharge charged on execution
    /// * `rank_by_effective_gas`: use effective gas in the fee/gas ranking
    ///
    /// # Returns
    /// A vector of `AsyncMessage` to execute
    pub fn take_batch_to_execute(
        &mut self,
        slot: Slot,
        max_gas: u64,
        async_msg_cst_gas_cost: u64,
        rank_by_effective_gas: bool,
    ) -> Vec<(AsyncMessageId, AsyncMessage)> {
        let mut available_gas = max_gas;

        // Choose which messages to take based on the message_cache
        // (all messages are considered: finals, in active_history and in speculative)
        let mut candidates: Vec<(AsyncMessageId, &AsyncMessage)> = self
            .message_cache
            .iter()
            .filter(|(_, message)| {
                Self::is_message_ready_to_execute(
                    &slot,
                    &message.validity_start,
                    &message.validity_end,
                ) && message.can_be_executed
            })
            .map(|(id, message)| (*id, message))
            .collect();

        if rank_by_effective_gas {
            candidates
                .sort_by_key(|(_, message)| message.compute_priority_id(async_msg_cst_gas_cost));
        }
        // else: BTreeMap iteration already yields stored AsyncMessageId order

        let mut wanted_ids = Vec::new();
        for (message_id, message) in candidates {
            let corrected_max_gas = message.max_gas.saturating_add(async_msg_cst_gas_cost);
            // Note: SecureShareOperation.get_validity_range(...) returns RangeInclusive
            //       so to be consistent here, use >= & <= checks
            if available_gas >= corrected_max_gas {
                available_gas -= corrected_max_gas;
                wanted_ids.push(message_id);
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

        taken_msgs
    }

    /// Settle a slot.
    /// Consume newly emitted messages into `self.async_pool`, recording changes into `self.settled_changes`.
    ///
    /// # Arguments
    /// * slot: slot that is being settled
    /// * ledger_changes: ledger changes for that slot, used to see if we can activate some messages
    /// * async_msg_cst_gas_cost: fixed per-message gas surcharge (used when ranking for eviction)
    /// * rank_by_effective_gas: when true, truncate by effective fee/gas (MIP-0002)
    ///
    /// # Returns
    /// the list of deleted `(message_id, message)`, used for reimbursement
    pub fn settle_slot(
        &mut self,
        slot: &Slot,
        ledger_changes: &LedgerChanges,
        async_msg_cst_gas_cost: u64,
        rank_by_effective_gas: bool,
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
        if rank_by_effective_gas {
            // Same ordering key as take_batch_to_execute: lowest effective priority last
            let mut ranked: Vec<_> = self
                .message_cache
                .iter()
                .map(|(id, msg)| (*id, msg.compute_priority_id(async_msg_cst_gas_cost)))
                .collect();
            ranked.sort_by_key(|(_, prio)| *prio);
            for (id, _) in ranked.into_iter().rev().take(excess_count) {
                eliminated_msgs.push((
                    id,
                    self.message_cache
                        .remove(&id)
                        .expect("message listed for eviction must be in cache"),
                ));
            }
        } else {
            for _ in 0..excess_count {
                eliminated_msgs.push(self.message_cache.pop_last().unwrap()); // will not panic (checked at excess_count computation)
            }
        }

        // Activate the messages that can be activated (triggered)
        //
        // Note: arming is intentionally not restricted to the message validity interval. A trigger
        //       observed before `validity_start` arms the message for good, and it then executes at
        //       the first slot of its validity interval that has gas available. This is wanted: a
        //       message is armed by an event that happened, and consistently with the fact that an
        //       armed message can never be disarmed, that event is not forgotten just because it
        //       came early. The validity interval bounds execution, not observation of the trigger.
        //
        // Note: activation happens here, i.e. after `take_batch_to_execute` already selected the
        //       messages to execute at this slot, so a message armed at slot S is executable from
        //       S+1 on. A trigger first observed at `validity_end` therefore never executes: the
        //       message is armed and then expires at the next slot, its coins being reimbursed by
        //       `cancel_async_message`. This cannot be avoided, since the ledger writes that arm
        //       the message are only known once the slot has been executed.
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
    use massa_models::address::Address;
    use massa_models::amount::Amount;
    use std::str::FromStr;

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

    fn test_pool() -> SpeculativeAsyncPool {
        SpeculativeAsyncPool {
            async_pool_max_length: 100,
            pool_changes: Default::default(),
            message_cache: Default::default(),
        }
    }

    fn test_message(emission_index: u64, max_gas: u64, fee_raw: u64) -> AsyncMessage {
        let addr =
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
        AsyncMessage::new(
            Slot::new(1, 0),
            emission_index,
            addr,
            addr,
            String::from("recv"),
            max_gas,
            Amount::from_raw(fee_raw),
            Amount::from_raw(0),
            Slot::new(0, 0),
            Slot::new(10, 0),
            vec![],
            None,
            Some(true),
        )
    }

    // Low-max_gas messages look better under fee/max_gas, but worse once the fixed
    // async_msg_cst_gas_cost is included — the MIP-0002 ranking must prefer the latter.
    //
    // Values are ABI-reachable: `send_message` rejects `max_gas < max_instance_cost` (2.1M).
    // Old ratio 25_000/2.1M ≈ 0.0119 vs 1_000_000/100M = 0.0100 (attacker first);
    // new ratio 25_000/3.1M ≈ 0.00806 vs 1_000_000/101M ≈ 0.0099 (honest first).
    const MAX_INSTANCE_COST: u64 = 2_100_000;
    const ATTACKER_MAX_GAS: u64 = MAX_INSTANCE_COST;
    const ATTACKER_FEE: u64 = 25_000;
    const HONEST_MAX_GAS: u64 = 100_000_000;
    const HONEST_FEE: u64 = 1_000_000;

    #[test]
    fn test_take_batch_effective_gas_priority() {
        let cst = 1_000_000u64;
        let cheap_looking = test_message(0, ATTACKER_MAX_GAS, ATTACKER_FEE);
        let honest = test_message(1, HONEST_MAX_GAS, HONEST_FEE);

        assert!(
            cheap_looking.compute_id() < honest.compute_id(),
            "without the surcharge, the low-max_gas message ranks first"
        );
        assert!(
            honest.compute_priority_id(cst) < cheap_looking.compute_priority_id(cst),
            "with the surcharge, the honest message ranks first"
        );

        let slot = Slot::new(1, 0);
        // Budget fits only one of the two once the fixed cost is added to each
        let budget = honest.max_gas.saturating_add(cst);

        let mut pool_legacy = test_pool();
        pool_legacy
            .message_cache
            .insert(cheap_looking.compute_id(), cheap_looking.clone());
        pool_legacy
            .message_cache
            .insert(honest.compute_id(), honest.clone());
        let taken_legacy = pool_legacy.take_batch_to_execute(slot, budget, cst, false);
        assert_eq!(taken_legacy.len(), 1);
        assert_eq!(
            taken_legacy[0].1.emission_index, 0,
            "pre-MIP ranking prefers the low-max_gas message"
        );

        let mut pool_mip = test_pool();
        pool_mip
            .message_cache
            .insert(cheap_looking.compute_id(), cheap_looking);
        pool_mip.message_cache.insert(honest.compute_id(), honest);
        let taken_mip = pool_mip.take_batch_to_execute(slot, budget, cst, true);
        assert_eq!(taken_mip.len(), 1);
        assert_eq!(
            taken_mip[0].1.emission_index, 1,
            "MIP-0002 ranking prefers the better effective fee/gas"
        );
    }

    #[test]
    fn test_settle_slot_effective_gas_eviction() {
        let cst = 1_000_000u64;
        let cheap_looking = test_message(0, ATTACKER_MAX_GAS, ATTACKER_FEE);
        let honest = test_message(1, HONEST_MAX_GAS, HONEST_FEE);

        let mut pool = test_pool();
        pool.async_pool_max_length = 1;
        pool.message_cache
            .insert(cheap_looking.compute_id(), cheap_looking.clone());
        pool.message_cache
            .insert(honest.compute_id(), honest.clone());

        let eliminated = pool.settle_slot(&Slot::new(1, 0), &LedgerChanges::default(), cst, true);
        assert_eq!(eliminated.len(), 1);
        assert_eq!(
            eliminated[0].1.emission_index, 0,
            "MIP-0002 eviction drops the worse effective fee/gas message"
        );
        assert_eq!(pool.message_cache.len(), 1);
        assert!(pool.message_cache.contains_key(&honest.compute_id()));
    }
}
