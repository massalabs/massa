//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final state of the node, which includes
//! the final ledger and asynchronous message pool that are kept at
//! the output of a given final slot (the latest executed final slot),
//! and need to be bootstrapped by nodes joining the network.

use crate::{config::FinalStateConfig, error::FinalStateError, state_changes::StateChanges};
use massa_async_pool::{AsyncMessageId, AsyncPool, AsyncPoolChanges, Change};
use massa_ledger_exports::{LedgerChanges, LedgerController};
use massa_models::{constants::THREAD_COUNT, Address, Slot};
use std::collections::VecDeque;

/// Represents a final state `(ledger, async pool)`
#[derive(Debug)]
pub struct FinalState {
    /// execution state configuration
    pub(crate) config: FinalStateConfig,
    /// slot at the output of which the state is attached
    pub slot: Slot,
    /// final ledger associating addresses to their balance, executable bytecode and data
    pub ledger: Box<dyn LedgerController>,
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
    pub fn new(
        config: FinalStateConfig,
        ledger: Box<dyn LedgerController>,
    ) -> Result<Self, FinalStateError> {
        // attach at the output of the latest initial final slot, that is the last genesis slot
        let slot = Slot::new(0, config.thread_count.saturating_sub(1));

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
        self.ledger
            .apply_changes(changes.ledger_changes.clone(), self.slot);
        self.async_pool
            .apply_changes_unchecked(&changes.async_pool_changes);

        // push history element and limit history size
        if self.config.final_history_length > 0 {
            while self.changes_history.len() >= self.config.final_history_length {
                self.changes_history.pop_front();
            }
            self.changes_history.push_back((slot, changes));
        }
    }

    /// Used for bootstrap
    /// Take a part of the final state changes (ledger and async pool) using a `Slot`, a `Address` and a `AsyncMessageId`.
    /// Every ledgers changes that are after `last_slot` and before or equal of `last_address` must be returned.
    /// Every async pool changes that are after `last_slot` and before or equal of `last_id_async_pool` must be returned.
    ///
    /// Error case: When the last_slot is too old for `self.changes_history`
    pub fn get_state_changes_part(
        &self,
        last_slot: Slot,
        last_address: Address,
        last_id_async_pool: AsyncMessageId,
    ) -> Result<StateChanges, FinalStateError> {
        let pos_slot = if !self.changes_history.is_empty() {
            // Safe because we checked that there is changes just above.
            let index = last_slot
                .slots_since(&self.changes_history[0].0, THREAD_COUNT)
                .map_err(|_| {
                    FinalStateError::LedgerError("Last slot is overflowing history.".to_string())
                })?;
            // Check if `last_slot` isn't in the future
            if self.changes_history.len() as u64 <= index {
                return Err(FinalStateError::LedgerError(
                    "Last slot is overflowing history.".to_string(),
                ));
            }
            index
        } else {
            return Ok(StateChanges::default());
        };
        let mut res_changes: StateChanges = StateChanges::default();
        for (_, changes) in self.changes_history.range((pos_slot as usize)..) {
            //Get ledger change that concern address <= last_address.
            let ledger_changes: LedgerChanges = LedgerChanges(
                changes
                    .ledger_changes
                    .0
                    .iter()
                    .filter_map(|(address, change)| {
                        if *address <= last_address {
                            Some((*address, change.clone()))
                        } else {
                            None
                        }
                    })
                    .collect(),
            );
            res_changes.ledger_changes = ledger_changes;

            //Get async pool changes that concern ids <= last_id_async_pool
            let async_pool_changes: AsyncPoolChanges = AsyncPoolChanges(
                changes
                    .async_pool_changes
                    .0
                    .iter()
                    .filter_map(|change| match change {
                        Change::Add(id, _) if id <= &last_id_async_pool => Some(change.clone()),
                        Change::Delete(id) if id <= &last_id_async_pool => Some(change.clone()),
                        Change::Add(..) => None,
                        Change::Delete(..) => None,
                    })
                    .collect(),
            );
            res_changes.async_pool_changes = async_pool_changes;
        }
        Ok(res_changes)
    }
}

#[cfg(test)]
mod tests {

    use std::collections::VecDeque;

    use crate::{FinalState, StateChanges};
    use massa_async_pool::test_exports::get_random_message;
    use massa_ledger_exports::SetUpdateOrDelete;
    use massa_models::{Address, Slot};
    use massa_signature::KeyPair;

    fn get_random_address() -> Address {
        let keypair = KeyPair::generate();
        Address::from_public_key(&keypair.get_public_key())
    }

    #[test]
    fn get_state_changes_part() {
        let message = get_random_message();
        // Building the state changes
        let mut history_state_changes: VecDeque<(Slot, StateChanges)> = VecDeque::new();
        let (low_address, high_address) = {
            let address1 = get_random_address();
            let address2 = get_random_address();
            if address1 < address2 {
                (address1, address2)
            } else {
                (address2, address1)
            }
        };
        let mut state_changes = StateChanges::default();
        state_changes
            .ledger_changes
            .0
            .insert(low_address, SetUpdateOrDelete::Delete);
        state_changes
            .async_pool_changes
            .0
            .push(massa_async_pool::Change::Add(
                message.compute_id(),
                message.clone(),
            ));
        history_state_changes.push_front((Slot::new(3, 0), state_changes));
        let mut state_changes = StateChanges::default();
        state_changes
            .ledger_changes
            .0
            .insert(high_address, SetUpdateOrDelete::Delete);
        history_state_changes.push_front((Slot::new(2, 0), state_changes.clone()));
        history_state_changes.push_front((Slot::new(1, 0), state_changes));
        let mut final_state: FinalState = Default::default();
        final_state.changes_history = history_state_changes;
        // Test slot filter
        let part = final_state
            .get_state_changes_part(Slot::new(2, 0), low_address, message.compute_id())
            .unwrap();
        assert_eq!(part.ledger_changes.0.len(), 1);
        // Test address filter
        let part = final_state
            .get_state_changes_part(Slot::new(2, 0), high_address, message.compute_id())
            .unwrap();
        assert_eq!(part.ledger_changes.0.len(), 1);
    }
}
