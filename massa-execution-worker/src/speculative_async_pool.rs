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
    /// List of changes that were applied to this SpeculativeAsyncPool since its creation
    changes: AsyncPoolChanges,
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
            changes: AsyncPoolChanges::default(),
        }
    }

    /// Returns the changes caused to the SpeculativeAsyncPool since its creation,
    /// and resets their local value to nothing.
    pub fn take(&mut self) -> AsyncPoolChanges {
        std::mem::take(&mut self.changes)
    }

    /// Takes a snapshot (clone) of the changes caused to the SpeculativeAsyncPool since its creation
    pub fn get_snapshot(&self) -> AsyncPoolChanges {
        self.changes.clone()
    }

    /// Resets the SpeculativeAsyncPool to a snapshot (see get_snapshot method)
    pub fn reset_to_snapshot(&mut self, snapshot: AsyncPoolChanges) {
        self.changes = snapshot;
    }

    /// Add a new message to the list of changes of this SpeculativeAsyncPool
    pub fn push_new_message(&mut self, msg: AsyncMessage) {
        self.changes.push_add(msg.compute_id(), msg);
    }

    /// Compute the AsyncPool, add the eliminated message IDs to the SpeculativeAsyncPool changes list
    /// and return the eliminated messages information for reimbursements
    pub fn compute_and_add_changes(&mut self, slot: Slot) -> Vec<(AsyncMessageId, AsyncMessage)> {
        let eliminated = self.async_pool.settle_slot(slot, self.changes.get_add());
        for v in &eliminated {
            self.changes.push_delete(v.0);
        }
        eliminated
    }
}
