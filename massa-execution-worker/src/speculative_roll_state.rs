// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::sync::Arc;

use massa_models::{BlockId, Slot};
use massa_pos_exports::{PoSChanges, SelectorController};
use parking_lot::RwLock;

use crate::active_history::ActiveHistory;

/// Speculative state of the rolls
pub(crate) struct SpeculativeRollState {
    /// Selector used to feed_cycle and get_selection
    selector: Box<dyn SelectorController>,
    /// History of the outputs of recently executed slots.
    /// Slots should be consecutive, newest at the back.
    active_history: Arc<RwLock<ActiveHistory>>,
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
        selector: Box<dyn SelectorController>,
        active_history: Arc<RwLock<ActiveHistory>>,
    ) -> Self {
        SpeculativeRollState {
            selector,
            active_history,
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

    /// Settle a slot.
    ///
    /// Compute the changes to be made on the roll state at the given slot.
    pub fn process_block(&mut self, block_id: Option<BlockId>) {
        // note: tmp dummy implementation
        // note: will be used only on real execution
        match block_id {
            Some(_id) => self
                .added_changes
                .production_stats
                .block_success_count
                .saturating_add(1),
            None => self
                .added_changes
                .production_stats
                .block_failure_count
                .saturating_add(1),
        };
    }

    pub fn settle_slot(&mut self, _slot: Slot) {
        // note: will be used on every kind of execution
    }
}
