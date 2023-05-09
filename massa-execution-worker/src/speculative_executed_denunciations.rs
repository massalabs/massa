//! Copyright (c) 2023 MASSA LABS <info@massa.net>

//! Speculative list of previously executed denunciations, to prevent reuse.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::active_history::{ActiveHistory, HistorySearchResult};
use massa_executed_ops::ExecutedDenunciationsChanges;
use massa_final_state::FinalState;
use massa_models::denunciation::DenunciationIndex;

/// Speculative state of executed denunciations
pub(crate) struct SpeculativeExecutedDenunciations {
    /// Thread-safe shared access to the final state. For reading only.
    final_state: Arc<RwLock<FinalState>>,

    /// History of the outputs of recently executed slots.
    /// Slots should be consecutive, newest at the back.
    active_history: Arc<RwLock<ActiveHistory>>,

    /// executed operations: maps the operation ID to its validity slot end - included
    executed_denunciations: ExecutedDenunciationsChanges,
}

impl SpeculativeExecutedDenunciations {
    /// Creates a new `SpeculativeExecutedDenunciations`
    ///
    /// # Arguments
    /// * `final_state`: thread-safe shared access the the final state
    /// * `active_history`: thread-safe shared access the speculative execution history
    pub(crate)  fn new(
        final_state: Arc<RwLock<FinalState>>,
        active_history: Arc<RwLock<ActiveHistory>>,
    ) -> Self {
        Self {
            final_state,
            active_history,
            executed_denunciations: Default::default(),
        }
    }

    /// Returns the set of operation IDs caused to the `SpeculativeExecutedDenunciations` since
    /// its creation, and resets their local value to nothing
    pub(crate)  fn take(&mut self) -> ExecutedDenunciationsChanges {
        std::mem::take(&mut self.executed_denunciations)
    }

    /// Takes a snapshot (clone) of the changes since its creation
    pub(crate)  fn get_snapshot(&self) -> ExecutedDenunciationsChanges {
        self.executed_denunciations.clone()
    }

    /// Resets the `SpeculativeRollState` to a snapshot (see `get_snapshot` method)
    pub(crate)  fn reset_to_snapshot(&mut self, snapshot: ExecutedDenunciationsChanges) {
        self.executed_denunciations = snapshot;
    }

    /// Checks if a denunciation was executed previously
    pub(crate)  fn is_denunciation_executed(&self, de_idx: &DenunciationIndex) -> bool {
        // check in the current changes
        if self.executed_denunciations.contains(de_idx) {
            return true;
        }

        // check in the active history, backwards
        match self
            .active_history
            .read()
            .fetch_executed_denunciation(de_idx)
        {
            HistorySearchResult::Present(_) => {
                return true;
            }
            HistorySearchResult::Absent => {
                return false;
            }
            HistorySearchResult::NoInfo => {}
        }

        // check in the final state
        self.final_state
            .read()
            .executed_denunciations
            .contains(de_idx)
    }

    /// Insert an executed denunciation.
    pub(crate)  fn insert_executed_denunciation(&mut self, de_idx: DenunciationIndex) {
        self.executed_denunciations.insert(de_idx);
    }
}
