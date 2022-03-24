// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative async pool represents the state of
//! the pool at an arbitrary execution slot.
//! It holds a copy of the final async pool to
//! which and keeps track of all the changes that
//! were applied to it since its creation.

use massa_async_pool::{AsyncMessage, AsyncMessageId, AsyncPool, AsyncPoolChanges};
use massa_models::Slot;

pub struct SpeculativeAsyncPool {
    async_pool: AsyncPool,
    changes: AsyncPoolChanges,
}

impl SpeculativeAsyncPool {
    pub fn new(mut async_pool: AsyncPool, previous_changes: AsyncPoolChanges) -> Self {
        async_pool.apply_changes_unchecked(previous_changes);
        SpeculativeAsyncPool {
            async_pool,
            changes: AsyncPoolChanges::default(),
        }
    }

    pub fn take(&mut self) -> AsyncPoolChanges {
        std::mem::take(&mut self.changes)
    }

    pub fn get_snapshot(&self) -> AsyncPoolChanges {
        self.changes.clone()
    }

    pub fn reset_to_snapshot(&mut self, snapshot: AsyncPoolChanges) {
        self.changes = snapshot;
    }

    pub fn push_new_message(&mut self, msg: AsyncMessage) {
        tracing::warn!("1 NEW MESSAGE PUSHED: {:?}", msg);
        self.changes.push_add(msg.compute_id(), msg);
    }

    pub fn compute_and_add_changes(&mut self, slot: Slot) -> Vec<(AsyncMessageId, AsyncMessage)> {
        let eliminated = self.async_pool.settle_slot(slot, self.changes.get_add());
        for v in &eliminated {
            self.changes.push_delete(v.0);
        }
        eliminated
    }
}
