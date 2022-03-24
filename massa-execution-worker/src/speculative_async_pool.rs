// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative async pool represents the state of
//! the pool at an arbitrary execution slot.

use massa_async_pool::{AsyncMessage, AsyncMessageId, AsyncPool, AsyncPoolChanges};
use massa_models::Slot;

/// The SpeculativeAsyncPool holds a copy of the final state async pool
/// to which it applies the previous changes.
/// The SpeculativeAsyncPool manipulates this copy to compute the full pool
/// while keeping track of all the newly added changes.
pub struct SpeculativeAsyncPool {
    /// Copy of the final async pool with the previous changes applied
    async_pool: AsyncPool,

    /// List of newly emitted async messages
    emitted: Vec<AsyncMessage>,

    /// List of changes (additions/deletions) to the pool after settling emitted messages
    settled_changes: AsyncPoolChanges,
}

impl SpeculativeAsyncPool {
    /// Creates a new SpeculativeAsyncPool
    ///
    /// # Arguments
    /// * async_pool: a copy of the final state AsyncPool
    /// * previous_changes: accumulation of changes that previously happened to the async pool since finality
    pub fn new(mut async_pool: AsyncPool, previous_changes: AsyncPoolChanges) -> Self {
        async_pool.apply_changes_unchecked(previous_changes);
        SpeculativeAsyncPool {
            async_pool,
            emitted: Default::default(),
            settled_changes: Default::default(),
        }
    }

    /// Returns the changes caused to the SpeculativeAsyncPool since its creation,
    /// and resets their local value to nothing.
    /// This must be called after settle_emitted_messages()
    pub fn take(&mut self) -> AsyncPoolChanges {
        std::mem::take(&mut self.settled_changes)
    }

    /// Takes a snapshot (clone) of the emitted messages
    pub fn get_snapshot(&self) -> Vec<AsyncMessage> {
        self.emitted.clone()
    }

    /// Resets the SpeculativeAsyncPool emitted messages to a snapshot (see get_snapshot method)
    pub fn reset_to_snapshot(&mut self, snapshot: Vec<AsyncMessage>) {
        self.emitted = snapshot;
    }

    /// Add a new message to the list of changes of this SpeculativeAsyncPool
    pub fn push_new_message(&mut self, msg: AsyncMessage) {
        self.emitted.push(msg);
    }

    /// Settle a slot.
    /// Consume newly emitted messages into self.async_pool, recording changes into self.settled_changes
    ///
    /// # Arguments
    /// * slot: slot that is being settled
    ///
    /// # Returns
    /// the list of deleted (message_id, message), used for reimbursement
    pub fn settle_slot(&mut self, slot: Slot) -> Vec<(AsyncMessageId, AsyncMessage)> {
        let (settled_changes, deleted_messages) = self
            .async_pool
            .settle_slot(slot, std::mem::take(&mut self.emitted));
        self.settled_changes = settled_changes;
        deleted_messages
    }
}
