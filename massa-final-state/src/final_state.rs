//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final state of the node, which includes
//! the final ledger and asynchronous message pool that are kept at
//! the output of a given final slot (the latest executed final slot),
//! and need to be bootstrapped by nodes joining the network.

use crate::{config::FinalStateConfig, error::FinalStateError, state_changes::StateChanges};
use massa_async_pool::{AsyncMessageId, AsyncPool, AsyncPoolChanges, Change};
use massa_executed_ops::ExecutedOps;
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_ledger_exports::{Key as LedgerKey, LedgerChanges, LedgerController};
use massa_models::{slot::Slot, streaming_step::StreamingStep};
use massa_pos_exports::{DeferredCredits, PoSFinalState, SelectorController};
use std::collections::VecDeque;
use tracing::info;

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
    /// hash of the final state, it is computed on finality
    pub final_state_hash: Hash,
}

const FINAL_STATE_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

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
            config.pos_config.clone(),
            &config.initial_seed_string,
            &config.initial_rolls_path,
            selector,
            ledger.get_ledger_hash(),
        )
        .map_err(|err| FinalStateError::PosError(format!("PoS final state init error: {}", err)))?;

        // attach at the output of the latest initial final slot, that is the last genesis slot
        let slot = Slot::new(0, config.thread_count.saturating_sub(1));

        // create the async pool
        let async_pool = AsyncPool::new(config.async_pool_config.clone());

        // create a default executed ops
        let executed_ops = ExecutedOps::new(config.executed_ops_config.clone());

        // create the final state
        Ok(FinalState {
            slot,
            ledger,
            async_pool,
            pos_state,
            config,
            executed_ops,
            changes_history: Default::default(), // no changes in history
            final_state_hash: Hash::from_bytes(FINAL_STATE_HASH_INITIAL_BYTES),
        })
    }

    /// Reset the final state to the initial state.
    ///
    /// USED ONLY FOR BOOTSTRAP
    pub fn reset(&mut self) {
        self.slot = Slot::new(0, self.config.thread_count.saturating_sub(1));
        self.ledger.reset();
        self.async_pool = AsyncPool::new(self.async_pool.config.clone());
        self.pos_state.reset();
        self.executed_ops = ExecutedOps::new(self.executed_ops.config.clone());
        self.changes_history.clear();
        // reset the final state hash
        self.final_state_hash = Hash::from_bytes(FINAL_STATE_HASH_INITIAL_BYTES);
    }

    /// Compute the current state hash.
    ///
    /// Used when finalizing a slot.
    /// Slot information is only used for logging.
    pub fn compute_state_hash_at_slot(&mut self, slot: Slot) {
        // 1. init hash concatenation with the ledger hash
        let ledger_hash = self.ledger.get_ledger_hash();
        let mut hash_concat: Vec<u8> = ledger_hash.to_bytes().to_vec();
        // 2. async_pool hash
        hash_concat.extend(self.async_pool.hash.to_bytes());
        // 3. pos deferred_credit hash
        hash_concat.extend(self.pos_state.deferred_credits.hash.to_bytes());
        // 4. pos cycle history hashes, skip the bootstrap safety cycle if there is one
        let n = (self.pos_state.cycle_history.len() == self.config.pos_config.cycle_history_length)
            as usize;
        for cycle_info in self.pos_state.cycle_history.iter().skip(n) {
            hash_concat.extend(cycle_info.cycle_global_hash.to_bytes());
        }
        // 5. executed operations hash
        hash_concat.extend(self.executed_ops.hash.to_bytes());
        // 6. compute and save final state hash
        self.final_state_hash = Hash::compute_from(&hash_concat);
        info!(
            "final_state hash at slot {}: {}",
            slot, self.final_state_hash
        );
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

        // apply the state changes
        // unwrap is justified because every error in PoS `apply_changes` is critical
        self.ledger
            .apply_changes(changes.ledger_changes.clone(), self.slot);
        self.async_pool
            .apply_changes_unchecked(&changes.async_pool_changes);
        self.pos_state
            .apply_changes(changes.pos_changes.clone(), self.slot, true)
            .expect("could not settle slot in final state proof-of-stake");
        // TODO:
        // do not panic above, it might just mean that the lookback cycle is not available
        // bootstrap again instead
        self.executed_ops
            .apply_changes(changes.executed_ops_changes.clone(), self.slot);

        // push history element and limit history size
        if self.config.final_history_length > 0 {
            while self.changes_history.len() >= self.config.final_history_length {
                self.changes_history.pop_front();
            }
            self.changes_history.push_back((slot, changes));
        }

        // compute the final state hash
        self.compute_state_hash_at_slot(slot);

        // feed final_state_hash to the last cycle
        let cycle = slot.get_cycle(self.config.periods_per_cycle);
        self.pos_state
            .feed_cycle_state_hash(cycle, self.final_state_hash);
    }

    /// Used for bootstrap.
    ///
    /// Retrieves every:
    /// * ledger change that is after `slot` and before or equal to `ledger_step` key
    /// * ledger change if main bootstrap process is finished
    /// * async pool change that is after `slot` and before or equal to `pool_step` message id
    /// * async pool change if main bootstrap process is finished
    /// * proof-of-stake deferred credits change if main bootstrap process is finished
    /// * proof-of-stake deferred credits change that is after `slot` and before or equal to `credits_step` slot
    /// * proof-of-stake cycle history change if main bootstrap process is finished
    /// * executed ops change if main bootstrap process is finished
    ///
    /// Produces an error when the `slot` is too old for `self.changes_history`
    pub fn get_state_changes_part(
        &self,
        slot: Slot,
        ledger_step: StreamingStep<LedgerKey>,
        pool_step: StreamingStep<AsyncMessageId>,
        cycle_step: StreamingStep<u64>,
        credits_step: StreamingStep<Slot>,
        ops_step: StreamingStep<Slot>,
    ) -> Result<Vec<(Slot, StateChanges)>, FinalStateError> {
        let position_slot = if let Some((first_slot, _)) = self.changes_history.front() {
            // Safe because we checked that there is changes just above.
            let index = slot
                .slots_since(first_slot, self.config.thread_count)
                .map_err(|_| {
                    FinalStateError::InvalidSlot(
                        "get_state_changes_part given slot is overflowing history".to_string(),
                    )
                })?
                .saturating_add(1);

            // Check if the `slot` index isn't in the future
            if self.changes_history.len() as u64 <= index {
                return Err(FinalStateError::InvalidSlot(
                    "slot index is overflowing history".to_string(),
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
                    let ledger_changes: LedgerChanges = LedgerChanges(
                        changes
                            .ledger_changes
                            .0
                            .iter()
                            .filter_map(|(address, change)| {
                                if *address <= key.address {
                                    Some((*address, change.clone()))
                                } else {
                                    None
                                }
                            })
                            .collect(),
                    );
                    slot_changes.ledger_changes = ledger_changes;
                }
                StreamingStep::Finished(_) => {
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
                                Change::Add(id, _) | Change::Activate(id) | Change::Delete(id)
                                    if id <= &last_id =>
                                {
                                    Some(change.clone())
                                }
                                _ => None,
                            })
                            .collect(),
                    );
                    slot_changes.async_pool_changes = async_pool_changes;
                }
                StreamingStep::Finished(_) => {
                    slot_changes.async_pool_changes = changes.async_pool_changes.clone();
                }
                _ => (),
            }

            // Get PoS deferred credits changes that concern credits <= credits_step
            match credits_step {
                StreamingStep::Ongoing(cursor_slot) => {
                    let deferred_credits = DeferredCredits {
                        credits: changes
                            .pos_changes
                            .deferred_credits
                            .credits
                            .iter()
                            .filter_map(|(credits_slot, credits)| {
                                if *credits_slot <= cursor_slot {
                                    Some((*credits_slot, credits.clone()))
                                } else {
                                    None
                                }
                            })
                            .collect(),
                        ..Default::default()
                    };
                    slot_changes.pos_changes.deferred_credits = deferred_credits;
                }
                StreamingStep::Finished(_) => {
                    slot_changes.pos_changes.deferred_credits =
                        changes.pos_changes.deferred_credits.clone();
                }
                _ => (),
            }

            // Get PoS cycle changes if cycle history main bootstrap finished
            if cycle_step.finished() {
                slot_changes.pos_changes.seed_bits = changes.pos_changes.seed_bits.clone();
                slot_changes.pos_changes.roll_changes = changes.pos_changes.roll_changes.clone();
                slot_changes.pos_changes.production_stats =
                    changes.pos_changes.production_stats.clone();
            }

            // Get executed operations changes if executed ops main bootstrap finished
            if ops_step.finished() {
                slot_changes.executed_ops_changes = changes.executed_ops_changes.clone();
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
        let message = get_random_message(None);
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
