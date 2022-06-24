// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_pos_exports::{PoSChanges, SelectorController};

/// Speculative state of the rolls
pub(crate) struct SpeculativeRollState {
    /// Selector used to feed_cycle and get_selection
    selector: Box<dyn SelectorController>,
    /// List of changes to the state after settling roll sell/buy
    settled_changes: PoSChanges,
}
