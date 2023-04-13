// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative asynchronous pool represents the state of
//! the pool at an arbitrary execution slot.

use crate::active_history::ActiveHistory;
use massa_async_pool::{AsyncMessage, AsyncMessageId, AsyncPool, AsyncPoolChanges};
use massa_final_state::FinalState;
use massa_ledger_exports::{LedgerBatch, LedgerChanges};
use massa_models::slot::Slot;
use parking_lot::RwLock;
use std::sync::Arc;

/// The `SpeculativeAsyncPool` holds a copy of the final state asynchronous pool
/// to which it applies the previous changes.
/// The `SpeculativeAsyncPool` manipulates this copy to compute the full pool
/// while keeping track of all the newly added changes.
pub(crate) struct SpeculativeAsyncPool {
    /// Copy of the final asynchronous pool with the previous changes applied
    async_pool: AsyncPool,

    /// List of newly emitted asynchronous messages
    emitted: Vec<(AsyncMessageId, AsyncMessage)>,

    /// List of changes (additions/deletions/activation) to the pool after settling emitted messages
    settled_changes: AsyncPoolChanges,
}

impl SpeculativeAsyncPool {
    /// Creates a new `SpeculativeAsyncPool`
    ///
    /// # Arguments
    pub fn new(
        final_state: Arc<RwLock<FinalState>>,
        active_history: Arc<RwLock<ActiveHistory>>,
    ) -> Self {
        // deduce speculative async pool from history
        let async_pool = final_state.read().async_pool.clone();
        for history_item in active_history.read().0.iter() {
            let mut batch = LedgerBatch::new(None, Some(async_pool.get_hash()));
            async_pool.apply_changes_unchecked_to_batch(
                &history_item.state_changes.async_pool_changes,
                &mut batch,
            );
            async_pool.write_batch(batch);
        }

        SpeculativeAsyncPool {
            async_pool,
            emitted: Default::default(),
            settled_changes: Default::default(),
        }
    }

    /// Returns the changes caused to the `SpeculativeAsyncPool` since its creation,
    /// and resets their local value to nothing.
    /// This must be called after `settle_emitted_messages()`
    pub fn take(&mut self) -> AsyncPoolChanges {
        std::mem::take(&mut self.settled_changes)
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
        // take a batch of messages, removing it from the async pool
        let msgs = self.async_pool.take_batch_to_execute(slot, max_gas);

        // settle deletions
        for (msg_id, _msg) in &msgs {
            self.settled_changes.push_delete(*msg_id);
        }

        msgs
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
        let (deleted_messages, triggered_messages) =
            self.async_pool
                .settle_slot(slot, &mut self.emitted, ledger_changes);
        for (msg_id, msg) in std::mem::take(&mut self.emitted) {
            self.settled_changes.push_add(msg_id, msg);
        }
        for (msg_id, _msg) in deleted_messages.iter() {
            self.settled_changes.push_delete(*msg_id);
        }
        for (msg_id, _msg) in triggered_messages.iter() {
            self.settled_changes.push_activate(*msg_id);
        }
        deleted_messages
    }
}
