// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::sync::Arc;

use massa_execution_exports::ExecutionError;
use massa_final_state::FinalState;
use massa_models::{Address, Amount, Slot};
use massa_pos_exports::{PoSChanges, SelectorController};
use parking_lot::RwLock;

use crate::active_history::ActiveHistory;

/// Speculative state of the rolls
#[allow(dead_code)]
pub(crate) struct SpeculativeRollState {
    /// Thread-safe shared access to the final state. For reading only.
    final_state: Arc<RwLock<FinalState>>,

    /// History of the outputs of recently executed slots.
    /// Slots should be consecutive, newest at the back.
    active_history: Arc<RwLock<ActiveHistory>>,

    /// Selector used to feed_cycle and get_selection
    selector: Box<dyn SelectorController>,

    /// List of changes to the state after settling roll sell/buy
    added_changes: PoSChanges,
}

impl SpeculativeRollState {
    /// Creates a new `SpeculativeRollState`
    ///
    /// # Arguments
    /// * `selector`: PoS draws selector controller
    /// * `active_history`: thread-safe shared access the speculative execution history
    pub fn new(
        final_state: Arc<RwLock<FinalState>>,
        active_history: Arc<RwLock<ActiveHistory>>,
        selector: Box<dyn SelectorController>,
    ) -> Self {
        SpeculativeRollState {
            final_state,
            active_history,
            selector,
            added_changes: Default::default(),
        }
    }

    /// Returns the changes caused to the `SpeculativeRollState` since its creation,
    /// and resets their local value to nothing.
    pub fn take(&mut self) -> PoSChanges {
        std::mem::take(&mut self.added_changes)
    }

    /// Takes a snapshot (clone) of the changes caused to the `SpeculativeRollState` since its creation
    pub fn get_snapshot(&self) -> PoSChanges {
        self.added_changes.clone()
    }

    /// Resets the `SpeculativeRollState` to a snapshot (see `get_snapshot` method)
    pub fn reset_to_snapshot(&mut self, snapshot: PoSChanges) {
        self.added_changes = snapshot;
    }

    /// Add `roll_count` rolls to the buyer address.
    /// Validity checks must be performed _outside_ of this function.
    ///
    /// # Arguments
    /// * `buyer_addr`: address that will receive the rolls
    /// * `roll_count`: number of rolls it will receive
    pub fn add_rolls(&mut self, buyer_addr: &Address, roll_count: u64) {
        let count = self
            .added_changes
            .roll_changes
            .entry(*buyer_addr)
            .or_insert_with(|| {
                self.active_history
                    .read()
                    .fetch_roll_count(buyer_addr)
                    .unwrap_or_else(|| self.final_state.read().pos_state.get_rolls_for(buyer_addr))
            });
        *count = count.saturating_add(roll_count);
    }

    /// Try to sell `roll_count` rolls from the seller address.
    ///
    /// # Arguments
    /// * `seller_addr`: address to sell the rolls from
    /// * `roll_count`: number of rolls to sell
    pub fn try_sell_rolls(
        &mut self,
        seller_addr: &Address,
        slot: Slot,
        roll_price: Amount,
        roll_count: u64,
    ) -> Result<(), ExecutionError> {
        // take a read lock on the final state
        let final_lock = self.final_state.read();

        // fetch the roll count from: current changes > active history > final state
        let count = self
            .added_changes
            .roll_changes
            .entry(*seller_addr)
            .or_insert_with(|| {
                self.active_history
                    .read()
                    .fetch_roll_count(seller_addr)
                    .unwrap_or_else(|| final_lock.pos_state.get_rolls_for(seller_addr))
            });

        // verify that the seller has enough rolls to sell
        if *count < roll_count {
            return Err(ExecutionError::RollsError(
                "not enough rolls to sell".to_string(),
            ));
        }

        // remove the rolls
        *count = count.saturating_sub(roll_count);

        // fetch the deferred credits from: current changes > active history > final state
        let credits = self
            .added_changes
            .deferred_credits
            .entry(slot)
            .or_insert_with(|| {
                self.active_history
                    .read()
                    .fetch_all_deferred_credits_at(&slot)
                    .into_iter()
                    .chain(final_lock.pos_state.get_deferred_credits_at(&slot))
                    .collect()
            });

        // add deferred reimbursement corresponding to the sold rolls value
        credits.insert(*seller_addr, roll_price.saturating_mul_u64(roll_count));

        Ok(())
    }

    /// Update production statistics of an address.
    ///
    /// # Arguments
    /// * `creator`: the supposed creator
    /// * `slot`: current slot
    /// * `contains_block`: indicates whether or not `creator` produced the block
    pub fn update_production_stats(
        &mut self,
        creator: &Address,
        slot: &Slot,
        contains_block: bool,
    ) {
        if let Some(production_stats) = self.added_changes.production_stats.get_mut(creator) {
            if contains_block {
                production_stats.block_success_count =
                    production_stats.block_success_count.saturating_add(1);
                self.added_changes.seed_bits.push(slot.get_first_bit());
            } else {
                production_stats.block_failure_count =
                    production_stats.block_failure_count.saturating_add(1);
            }
        }
    }
}
