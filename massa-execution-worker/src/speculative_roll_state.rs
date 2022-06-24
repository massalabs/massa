// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::sync::Arc;

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
    settled_changes: PoSChanges,
}
