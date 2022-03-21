// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! The speculative async pool represents the state of
//! the pool at an arbitrary execution slot.
//! It holds a copy of the final async pool to
//! which and keeps track of all the changes that
//! were applied to it since its creation.

use massa_async_pool::{AsyncMessage, AsyncPoolChanges};
use massa_final_state::FinalState;
use massa_models::{Amount, Slot};
use parking_lot::RwLock;
use std::sync::Arc;

pub struct SpeculativeAsyncPool {
    final_state: Arc<RwLock<FinalState>>,
    previous_changes: AsyncPoolChanges,
    added_changes: AsyncPoolChanges,
    new_messages: Vec<AsyncMessage>,
}

impl SpeculativeAsyncPool {
    pub fn new(final_state: Arc<RwLock<FinalState>>, previous_changes: AsyncPoolChanges) -> Self {
        SpeculativeAsyncPool {
            final_state,
            previous_changes,
            added_changes: Default::default(),
            new_messages: Vec::new(),
        }
    }

    pub fn take(&mut self) -> AsyncPoolChanges {
        std::mem::take(&mut self.added_changes)
    }

    pub fn get_snapshot(&self) -> AsyncPoolChanges {
        self.added_changes.clone()
    }

    pub fn reset_to_snapshot(&mut self, snapshot: AsyncPoolChanges) {
        self.added_changes = snapshot;
    }

    pub fn push_new_message(&mut self, msg: AsyncMessage) {
        self.new_messages.push(msg);
    }

    pub fn compute_and_add_changes(&mut self, slot: Slot) -> Vec<AsyncMessage> {
        let mut pool_copy = self.final_state.read().async_pool.clone();
        pool_copy.apply_changes_unchecked(std::mem::take(&mut self.previous_changes));

        let eliminated = pool_copy.settle_slot(slot, std::mem::take(&mut self.new_messages));
        for v in &eliminated {
            self.added_changes.push_delete(v.0);
        }
        let mut reimbursements = eliminated
            .into_iter()
            .map(|x| x.1)
            .collect::<Vec<AsyncMessage>>();
        for msg in &self.new_messages {
            if let Some(cost) = msg.gas_price.checked_mul_u64(msg.max_gas) {
                self.added_changes.push_add(
                    (
                        cost,
                        std::cmp::Reverse(msg.emission_slot),
                        std::cmp::Reverse(msg.emission_index),
                    ),
                    msg.clone(),
                );
            } else {
                reimbursements.push(msg.clone());
            }
        }
        reimbursements
    }
}
