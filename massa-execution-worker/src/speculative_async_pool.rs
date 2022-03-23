// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative async pool represents the state of
//! the pool at an arbitrary execution slot.
//! It holds a copy of the final async pool to
//! which and keeps track of all the changes that
//! were applied to it since its creation.

use massa_async_pool::{AsyncMessage, AsyncMessageId, AsyncPoolChanges};
use massa_final_state::FinalState;
use massa_models::Slot;
use parking_lot::RwLock;
use std::sync::Arc;

pub struct SpeculativeAsyncPool {
    final_state: Arc<RwLock<FinalState>>,
    previous_changes: AsyncPoolChanges,
    changes: AsyncPoolChanges,
}

impl SpeculativeAsyncPool {
    pub fn new(final_state: Arc<RwLock<FinalState>>, previous_changes: AsyncPoolChanges) -> Self {
        SpeculativeAsyncPool {
            final_state,
            previous_changes,
            changes: Default::default(),
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
        let mut pool_copy = self.final_state.read().async_pool.clone();
        pool_copy.apply_changes_unchecked(std::mem::take(&mut self.previous_changes));

        let eliminated = pool_copy.settle_slot(slot, self.changes.get_add());
        for v in &eliminated {
            self.changes.push_delete(v.0);
        }
        eliminated
    }
}
