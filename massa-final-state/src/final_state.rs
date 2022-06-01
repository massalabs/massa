//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final state of the node, which includes
//! the final ledger and asynchronous message pool that are kept at
//! the output of a given final slot (the latest executed final slot),
//! and need to be bootstrapped by nodes joining the network.

use crate::{config::FinalStateConfig, error::FinalStateError, state_changes::StateChanges};
use massa_async_pool::{AsyncMessageId, AsyncPool, AsyncPoolChanges, Change};
use massa_ledger::{FinalLedger, LedgerChanges};
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
            .apply_changes_at_slot(changes.ledger_changes.clone(), self.slot);
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
    /// Take a part of the final state changes (ledger and async pool) using a `Slot`, a `max_address` and a `AsyncMessageId`.
    /// Every ledgers changes that are after `min_slot` and below `end_cursor` must be returned.
    /// Every async pool changes that are after `min_slot` and below `max_id_async_pool` must be returned.
    pub fn get_part_state_changes(
        &self,
        min_slot: Option<Slot>,
        max_address: Option<Address>,
        max_id_async_pool: Option<AsyncMessageId>,
    ) -> Vec<StateChanges> {
        let pos_slot = min_slot
            .map(|min_slot| self.changes_history.partition_point(|(s, _)| s < &min_slot))
            .unwrap_or(0);
        let mut res_changes: Vec<StateChanges> = Vec::new();
        for (_, changes) in self.changes_history.range(pos_slot..) {
            let mut elem: StateChanges = StateChanges::default();
            //Get ledger change that concern address < max_address.
            let ledger_changes: LedgerChanges = LedgerChanges(
                changes
                    .ledger_changes
                    .0
                    .iter()
                    .filter_map(|(address, change)| match max_address {
                        // TODO: Improve by taking in count the step
                        Some(max_address) if max_address < *address => {
                            Some((*address, change.clone()))
                        }
                        Some(_) => None,
                        _ => Some((*address, change.clone())),
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
                    .filter_map(|change| {
                        if let Some(max_id_async_pool) = max_id_async_pool {
                            match change {
                                Change::Add(id, _) if id < &max_id_async_pool => {
                                    Some(change.clone())
                                }
                                Change::Delete(id) if id < &max_id_async_pool => {
                                    Some(change.clone())
                                }
                                _ => None,
                            }
                        } else {
                            Some(change.clone())
                        }
                    })
                    .collect(),
            );
            elem.async_pool_changes = async_pool_changes;
            if !elem.async_pool_changes.0.is_empty() || !elem.ledger_changes.0.is_empty() {
                res_changes.push(elem);
            }
        }
        res_changes
    }
}

#[cfg(test)]
mod tests {

    use std::collections::VecDeque;

    use crate::{FinalState, StateChanges};
    use massa_ledger::SetUpdateOrDelete;
    use massa_models::{Address, Slot};
    use massa_signature::{derive_public_key, generate_random_private_key};

    fn get_random_address() -> Address {
        let priv_key = generate_random_private_key();
        let pub_key = derive_public_key(&priv_key);
        Address::from_public_key(&pub_key)
    }

    #[test]
    fn get_part_state_changes() {
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
        let part = final_state.get_part_state_changes(Some(Slot::new(2, 0)), None, None);
        assert_eq!(part.len(), 2);
        // Test address filter
        let part =
            final_state.get_part_state_changes(Some(Slot::new(2, 0)), Some(low_address), None);
        assert_eq!(part.len(), 1);
    }
}
