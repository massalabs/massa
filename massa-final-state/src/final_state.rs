//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the execution state, which includes
//! the final ledger and async message pool that are kept at
//! the output of a given final slot,
//! and need to be bootstrapped by nodes joining the network.

use crate::{
    bootstrap::FinalStateBootstrap, config::FinalStateConfig, error::FinalStateError,
    state_changes::StateChanges,
};
use massa_ledger::FinalLedger;
use massa_models::Slot;
use std::collections::VecDeque;

/// Represents a final execution state (ledger, async pool)
pub struct FinalState {
    /// execution state config
    config: FinalStateConfig,
    /// slot at the output of which the state is attached
    pub slot: Slot,
    /// final ledger associating addresses to their balance, executable bytecode and data
    pub ledger: FinalLedger,
    /// history of recent final state changes, useful for streaming bootstrap
    /// front = oldest, back = newest
    changes_history: VecDeque<(Slot, StateChanges)>,
}

impl FinalState {
    /// Initializes a new FinalState
    ///
    /// # Arguments
    /// * config: the configuration of the execution state
    pub fn new(config: FinalStateConfig) -> Result<Self, FinalStateError> {
        // attach at the output of the latest initial final slot, that is the last genesis slot
        let slot = Slot::new(0, config.thread_count.saturating_sub(1));

        // load the initial final ledger from file
        let ledger = FinalLedger::new(config.ledger_config.clone()).map_err(|err| {
            FinalStateError::LedgerError(format!("could not initialize ledger: {}", err))
        })?;

        // generate the final ledger
        Ok(FinalState {
            slot,
            ledger,
            config,
            changes_history: Default::default(), // no changes in history
        })
    }

    /// Initialize a FinalState from a bootstrap state
    ///
    /// # Arguments
    /// * config: final state config
    /// * state: bootstrap state
    pub fn from_bootstrap_state(config: FinalStateConfig, state: FinalStateBootstrap) -> Self {
        FinalState {
            slot: state.slot,
            ledger: FinalLedger::from_bootstrap_state(config.ledger_config.clone(), state.ledger),
            config,
            changes_history: Default::default(), // no changes in history
        }
    }

    /// Gets a snapshot of the state to bootstrap other nodes
    pub fn get_bootstrap_state(&self) -> FinalStateBootstrap {
        FinalStateBootstrap {
            slot: self.slot,
            ledger: self.ledger.get_bootstrap_state(),
        }
    }

    /// Applies changes to the execution state at a given slot, and settles that slot forever.
    /// Once this is called, the state is attached at the output of the provided slot.
    ///
    /// Panics if the new slot is not the one coming just after the current one.
    pub fn settle_slot(&mut self, slot: Slot, changes: StateChanges) {
        // check slot consistency
        let next_slot = self
            .slot
            .get_next_slot(self.config.thread_count)
            .expect("overflow in execution state slot");
        if slot != next_slot {
            panic!("attempting to apply execution state changes at slot {} while the current slot is {}", slot, self.slot);
        }

        // update current slot
        self.slot = slot;

        // push history element and limit history size
        if self.config.final_history_length > 0 {
            while self.changes_history.len() >= self.config.final_history_length {
                self.changes_history.pop_front();
            }
            self.changes_history.push_back((slot, changes));
        }
    }
}
