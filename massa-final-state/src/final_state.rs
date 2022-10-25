//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final state of the node, which includes
//! the final ledger and asynchronous message pool that are kept at
//! the output of a given final slot (the latest executed final slot),
//! and need to be bootstrapped by nodes joining the network.

use crate::{
    config::FinalStateConfig, error::FinalStateError, state_changes::StateChanges, ExecutedOps,
};
use massa_async_pool::{AsyncMessageId, AsyncPool, AsyncPoolChanges, Change};
use massa_ledger_exports::{get_address_from_key, LedgerChanges, LedgerController};
use massa_models::{operation::OperationId, slot::Slot, streaming_step::StreamingStep};
use massa_pos_exports::{PoSFinalState, SelectorController};
use std::collections::VecDeque;
use tracing::debug;

/// Represents a final state `(ledger, async pool, executed_ops and the state of the PoS)`
pub struct FinalState {
    /// execution state configuration
    pub(crate) config: FinalStateConfig,
    /// slot at the output of which the state is attached
    pub slot: Slot,
    /// final ledger associating addresses to their balance, executable bytecode and data
    pub ledger: Box<dyn LedgerController>,
    /// asynchronous pool containing messages sorted by priority and their data
    pub async_pool: AsyncPool,
    /// proof of stake state containing cycle history and deferred credits
    pub pos_state: PoSFinalState,
    /// executed operations
    pub executed_ops: ExecutedOps,
    /// history of recent final state changes, useful for streaming bootstrap
    /// `front = oldest`, `back = newest`
    pub changes_history: VecDeque<(Slot, StateChanges)>,
}

impl FinalState {
    /// Initializes a new `FinalState`
    ///
    /// # Arguments
    /// * `config`: the configuration of the execution state
    pub fn new(
        config: FinalStateConfig,
        ledger: Box<dyn LedgerController>,
        selector: Box<dyn SelectorController>,
    ) -> Result<Self, FinalStateError> {
        // create the pos state
        let pos_state = PoSFinalState::new(
            &config.initial_seed_string,
            &config.initial_rolls_path,
            config.periods_per_cycle,
            config.thread_count,
            selector,
        )
        .map_err(|err| FinalStateError::PosError(format!("PoS final state init error: {}", err)))?;

        // attach at the output of the latest initial final slot, that is the last genesis slot
        let slot = Slot::new(0, config.thread_count.saturating_sub(1));

        // create the async pool
        let async_pool = AsyncPool::new(config.async_pool_config.clone());

        // create a default executed ops
        let executed_ops = ExecutedOps::default();

        // generate the final state
        Ok(FinalState {
            slot,
            ledger,
            async_pool,
            pos_state,
            config,
            executed_ops,
            changes_history: Default::default(), // no changes in history
        })
    }

    /// Performs the initial draws.
    pub fn compute_initial_draws(&mut self) -> Result<(), FinalStateError> {
        self.pos_state
            .compute_initial_draws()
            .map_err(|err| FinalStateError::PosError(err.to_string()))
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
        self.pos_state
            .apply_changes(changes.pos_changes.clone(), self.slot, true)
            .expect("could not settle slot in final state PoS"); //TODO do not panic here: it might just mean that the lookback cycle is not available
        self.executed_ops.extend(changes.executed_ops.clone());
        self.executed_ops.prune(self.slot);

        // push history element and limit history size
        if self.config.final_history_length > 0 {
            while self.changes_history.len() >= self.config.final_history_length {
                self.changes_history.pop_front();
            }
            self.changes_history.push_back((slot, changes));
        }

        debug!(
            "ledger hash at slot {}: {}",
            slot,
            self.ledger.get_ledger_hash()
        );
        debug!(
            "executed_ops hash at slot {}: {:?}",
            slot, self.executed_ops.hash
        );
    }

    /// Used for bootstrap.
    ///
    /// Retrieves every:
    /// * ledger change that is after `slot` and before or equal to `ledger_step` key
    /// * ledger change if main bootstrap process is finished
    /// * async pool change that is after `slot` and before or equal to `pool_step` message id
    /// * proof-of-stake change if main bootstrap process is finished
    /// * proof-of-stake change if main bootstrap process is finished
    /// * executed ops change if main bootstrap process is finished
    ///
    /// Produces an error when the `slot` is too old for `self.changes_history`
    pub fn get_state_changes_part(
        &self,
        slot: Slot,
        ledger_step: StreamingStep<Vec<u8>>,
        pool_step: StreamingStep<AsyncMessageId>,
        cycle_step: StreamingStep<u64>,
        credits_step: StreamingStep<Slot>,
        ops_step: StreamingStep<OperationId>,
    ) -> Result<Vec<(Slot, StateChanges)>, FinalStateError> {
        let position_slot = if let Some((first_slot, _)) = self.changes_history.front() {
            // Safe because we checked that there is changes just above.
            let index = slot
                .slots_since(first_slot, self.config.thread_count)
                .map_err(|_| {
                    FinalStateError::LedgerError(
                        "get_state_changes_part given slot is overflowing history.".to_string(),
                    )
                })?
                .saturating_add(1);

            // Check if the `slot` index isn't in the future
            if self.changes_history.len() as u64 <= index {
                return Err(FinalStateError::LedgerError(
                    "slot index is overflowing history.".to_string(),
                ));
            }
            index
        } else {
            return Ok(Vec::new());
        };
        let mut res_changes: Vec<(Slot, StateChanges)> = Vec::new();
        for (slot, changes) in self.changes_history.range((position_slot as usize)..) {
            let mut slot_changes = StateChanges::default();

            // Get ledger change that concern address <= ledger_step
            match ledger_step.clone() {
                StreamingStep::Ongoing(key) => {
                    let addr = get_address_from_key(&key).ok_or_else(|| {
                        FinalStateError::LedgerError(
                            "Invalid key in ledger streaming step".to_string(),
                        )
                    })?;
                    let ledger_changes: LedgerChanges = LedgerChanges(
                        changes
                            .ledger_changes
                            .0
                            .iter()
                            .filter_map(|(address, change)| {
                                if *address <= addr {
                                    Some((*address, change.clone()))
                                } else {
                                    None
                                }
                            })
                            .collect(),
                    );
                    slot_changes.ledger_changes = ledger_changes;
                }
                StreamingStep::Finished => {
                    slot_changes.ledger_changes = changes.ledger_changes.clone();
                }
                _ => (),
            }

            // Get async pool changes that concern ids <= pool_step
            match pool_step {
                StreamingStep::Ongoing(last_id) => {
                    let async_pool_changes: AsyncPoolChanges = AsyncPoolChanges(
                        changes
                            .async_pool_changes
                            .0
                            .iter()
                            .filter_map(|change| match change {
                                Change::Add(id, _) if id <= &last_id => Some(change.clone()),
                                Change::Delete(id) if id <= &last_id => Some(change.clone()),
                                Change::Add(..) => None,
                                Change::Delete(..) => None,
                            })
                            .collect(),
                    );
                    slot_changes.async_pool_changes = async_pool_changes;
                }
                StreamingStep::Finished => {
                    slot_changes.async_pool_changes = changes.async_pool_changes.clone();
                }
                _ => (),
            }

            // Get Proof of Stake state changes if current bootstrap cycle is incomplete (so last)
            if cycle_step.finished() && credits_step.finished() {
                slot_changes.pos_changes = changes.pos_changes.clone();
            }

            // Get executed operations changes if classic bootstrap finished
            if ops_step.finished() {
                slot_changes.executed_ops = changes.executed_ops.clone();
            }

            // Push the slot changes
            res_changes.push((*slot, slot_changes));
        }
        Ok(res_changes)
    }
}

#[cfg(test)]
mod tests {

    use std::collections::VecDeque;

    use crate::StateChanges;
    use massa_async_pool::test_exports::get_random_message;
    use massa_ledger_exports::SetUpdateOrDelete;
    use massa_models::{address::Address, slot::Slot};
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
            .push(massa_async_pool::Change::Add(message.compute_id(), message));
        history_state_changes.push_front((Slot::new(3, 0), state_changes));
        let mut state_changes = StateChanges::default();
        state_changes
            .ledger_changes
            .0
            .insert(high_address, SetUpdateOrDelete::Delete);
        history_state_changes.push_front((Slot::new(2, 0), state_changes.clone()));
        history_state_changes.push_front((Slot::new(1, 0), state_changes));
        // TODO: re-enable this test after refactoring is over
        // let mut final_state: FinalState = Default::default();
        // final_state.changes_history = history_state_changes;
        // // Test slot filter
        // let part = final_state
        //     .get_state_changes_part(Slot::new(2, 0), low_address, message.compute_id(), None)
        //     .unwrap();
        // assert_eq!(part.ledger_changes.0.len(), 1);
        // // Test address filter
        // let part = final_state
        //     .get_state_changes_part(Slot::new(2, 0), high_address, message.compute_id(), None)
        //     .unwrap();
        // assert_eq!(part.ledger_changes.0.len(), 1);
    }
}
