//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final state of the node, which includes
//! the final ledger and asynchronous message pool that are kept at
//! the output of a given final slot (the latest executed final slot),
//! and need to be bootstrapped by nodes joining the network.

use crate::{
    bootstrap::FinalStateBootstrap, config::FinalStateConfig, error::FinalStateError,
    state_changes::StateChanges,
};
use massa_async_pool::{AsyncMessageId, AsyncPool, AsyncPoolChanges, Change};
use massa_ledger::{Applicable, FinalLedger, LedgerChanges};
use massa_models::{Address, Slot};
use std::collections::VecDeque;

/// Represents a final state `(ledger, async pool)`
#[derive(Debug)]
pub struct FinalState {
    /// execution state configuration
    pub(crate) config: FinalStateConfig,
    /// slot at the output of which the state is attached
    pub slot: Slot,
    /// final ledger associating addresses to their balance, executable bytecode and data
    pub ledger: FinalLedger,
    /// asynchronous pool containing messages sorted by priority and their data
    pub async_pool: AsyncPool,
    /// history of recent final state changes, useful for streaming bootstrap
    /// `front = oldest`, `back = newest`
    pub(crate) changes_history: VecDeque<(Slot, StateChanges)>,
}

impl FinalState {
    /// Initializes a new `FinalState`
    ///
    /// # Arguments
    /// * `config`: the configuration of the execution state
    pub fn new(config: FinalStateConfig) -> Result<Self, FinalStateError> {
        // attach at the output of the latest initial final slot, that is the last genesis slot
        let slot = Slot::new(0, config.thread_count.saturating_sub(1));

        // load the initial final ledger from file
        let ledger = FinalLedger::new(config.ledger_config.clone()).map_err(|err| {
            FinalStateError::LedgerError(format!("could not initialize ledger: {}", err))
        })?;

        // create the async pool
        let async_pool = AsyncPool::new(config.async_pool_config.clone());

        // generate the final state
        Ok(FinalState {
            slot,
            ledger,
            async_pool,
            config,
            changes_history: Default::default(), // no changes in history
        })
    }

    /// Initialize a `FinalState` from a bootstrap state
    ///
    /// # Arguments
    /// * `config`: final state configuration
    /// * `state`: bootstrap state
    pub fn from_bootstrap_state(config: FinalStateConfig, state: FinalStateBootstrap) -> Self {
        FinalState {
            slot: state.slot,
            ledger: FinalLedger::from_bootstrap_state(config.ledger_config.clone(), state.ledger),
            async_pool: AsyncPool::from_bootstrap_snapshot(
                config.async_pool_config.clone(),
                state.async_pool,
            ),
            config,
            changes_history: Default::default(), // no changes in history
        }
    }

    /// Gets a snapshot of the state to bootstrap other nodes
    pub fn get_bootstrap_state(&self) -> FinalStateBootstrap {
        FinalStateBootstrap {
            slot: self.slot,
            async_pool: self.async_pool.get_bootstrap_snapshot(),
            ledger: self.ledger.get_bootstrap_state(),
        }
    }

    /// Applies changes to the execution state at a given slot, and settles that slot forever.
    /// Once this is called, the state is attached at the output of the provided slot.
    ///
    /// Panics if the new slot is not the one coming just after the current one.
    pub fn finalize(&mut self, slot: Slot, changes: StateChanges) {
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

        // apply changes
        self.ledger.apply(changes.ledger_changes.clone());
        self.async_pool
            .apply_changes_unchecked(changes.async_pool_changes.clone());

        // push history element and limit history size
        if self.config.final_history_length > 0 {
            while self.changes_history.len() >= self.config.final_history_length {
                self.changes_history.pop_front();
            }
            self.changes_history.push_back((slot, changes));
        }
    }

    /// Used for bootstrap
    /// TODO: Document
    pub fn get_part_state_changes(
        &self,
        slot: Slot,
        max_address: Address,
        max_id_async_pool: AsyncMessageId,
    ) -> Vec<(Slot, StateChanges)> {
        let pos_slot = self.changes_history.partition_point(|(s, _)| s > &slot);
        let mut res_changes: Vec<(Slot, StateChanges)> = Vec::new();
        for (slot, changes) in self.changes_history.range(pos_slot..) {
            let mut elem: StateChanges = StateChanges::default();

            //Get ledger change that concern address < max_address.
            let ledger_changes: LedgerChanges = LedgerChanges(
                changes
                    .ledger_changes
                    .0
                    .iter()
                    .filter_map(|(address, change)| {
                        if address < &max_address {
                            Some((*address, change.clone()))
                        } else {
                            None
                        }
                    })
                    .collect(),
            );
            elem.ledger_changes = ledger_changes;

            //Get async pool changes that concern ids < max_id_async_pool
            let async_pool_changes: AsyncPoolChanges = AsyncPoolChanges(
                changes
                    .async_pool_changes
                    .0
                    .iter()
                    .filter_map(|change| match change {
                        Change::Add(id, _) if id < &max_id_async_pool => Some(change.clone()),
                        Change::Delete(id) if id < &max_id_async_pool => Some(change.clone()),
                        _ => None,
                    })
                    .collect(),
            );
            elem.async_pool_changes = async_pool_changes;
            if !elem.async_pool_changes.0.is_empty() || !elem.ledger_changes.0.is_empty() {
                res_changes.push((*slot, elem));
            }
        }
        res_changes
    }
}
