//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final state of the node, which includes
//! the final ledger and asynchronous message pool that are kept at
//! the output of a given final slot (the latest executed final slot),
//! and need to be bootstrapped by nodes joining the network.

use crate::{config::FinalStateConfig, error::FinalStateError, state_changes::StateChanges};
use core::ops::Bound::{Excluded, Included};
use massa_async_pool::{AsyncMessageIdDeserializer, AsyncPool, AsyncPoolChanges};
use massa_db::{
    DBBatch, MassaDB, ASYNC_POOL_PREFIX, CF_ERROR, CYCLE_HISTORY_PREFIX, DEFERRED_CREDITS_PREFIX,
    EXECUTED_DENUNCIATIONS_PREFIX, EXECUTED_OPS_PREFIX, KEY_DESER_ERROR, LEDGER_PREFIX,
    MESSAGE_ID_DESER_ERROR, SLOT_DESER_ERROR, STATE_CF,
};
use massa_executed_ops::{ExecutedDenunciations, ExecutedOps};
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_ledger_exports::{KeyDeserializer, LedgerChanges, LedgerController};
use massa_models::config::PERIODS_BETWEEN_BACKUPS;
use massa_models::slot::SlotDeserializer;
use massa_models::{slot::Slot, streaming_step::StreamingStep};
use massa_pos_exports::{DeferredCredits, PoSFinalState, SelectorController};
use massa_serialization::{DeserializeError, Deserializer};
use parking_lot::RwLock;
use rocksdb::{Direction, IteratorMode};
use std::collections::BTreeMap;
use std::{collections::VecDeque, sync::Arc};
use tracing::{debug, info};

/// Represents a final state `(ledger, async pool, executed_ops, executed_de and the state of the PoS)`
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
    /// executed denunciations
    pub executed_denunciations: ExecutedDenunciations,
    /// history of recent final state changes, useful for streaming bootstrap
    /// `front = oldest`, `back = newest`
    pub changes_history: VecDeque<(Slot, StateChanges)>,
    /// hash of the final state, it is computed on finality
    pub final_state_hash: Hash,
    /// last_start_period
    /// * If start all new network: set to 0
    /// * If from snapshot: retrieve from args
    /// * If from bootstrap: set during bootstrap
    pub last_start_period: u64,
    /// the rocksdb instance used to write every final_state struct on disk
    pub db: Arc<RwLock<MassaDB>>,
}

const FINAL_STATE_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

impl FinalState {
    /// Initializes a new `FinalState`
    ///
    /// # Arguments
    /// * `config`: the configuration of the final state to use for initialization
    /// * `ledger`: the instance of the ledger on disk. Used to apply changes to the ledger.
    /// * `selector`: the pos selector. Used to send draw inputs when a new cycle is completed.
    pub fn new(
        db: Arc<RwLock<MassaDB>>,
        config: FinalStateConfig,
        ledger: Box<dyn LedgerController>,
        selector: Box<dyn SelectorController>,
        reset_final_state: bool,
    ) -> Result<Self, FinalStateError> {
        let state_slot = db.read().get_slot(config.thread_count);
        match state_slot {
            Ok(slot) => {
                info!(
                    "Recovered ledger. state slot: {}, state hash: {}",
                    slot,
                    db.read().get_db_hash()
                );
            }
            Err(_e) => {
                info!(
                    "Recovered ledger. Unknown state slot, state hash: {}",
                    db.read().get_db_hash()
                );
            }
        }

        // create the pos state
        let pos_state = PoSFinalState::new(
            config.pos_config.clone(),
            &config.initial_seed_string,
            &config.initial_rolls_path,
            selector,
            db.read().get_db_hash(),
            db.clone(),
        )
        .map_err(|err| FinalStateError::PosError(format!("PoS final state init error: {}", err)))?;

        // attach at the output of the latest initial final slot, that is the last genesis slot
        let slot = Slot::new(0, config.thread_count.saturating_sub(1));

        // create the async pool
        let async_pool = AsyncPool::new(config.async_pool_config.clone(), db.clone());

        // create a default executed ops
        let executed_ops = ExecutedOps::new(config.executed_ops_config.clone(), db.clone());

        // create a default executed denunciations
        let executed_denunciations =
            ExecutedDenunciations::new(config.executed_denunciations_config.clone(), db.clone());

        let mut final_state = FinalState {
            slot,
            ledger,
            async_pool,
            pos_state,
            config,
            executed_ops,
            executed_denunciations,
            changes_history: Default::default(), // no changes in history
            final_state_hash: Hash::from_bytes(FINAL_STATE_HASH_INITIAL_BYTES),
            last_start_period: 0,
            db,
        };

        if reset_final_state {
            final_state.async_pool.reset();
            final_state.pos_state.reset();
            final_state.executed_ops.reset();
            final_state.executed_denunciations.reset();
        }

        info!(
            "final_state hash at slot {}: {}",
            slot,
            final_state.db.read().get_db_hash()
        );

        // create the final state
        Ok(final_state)
    }

    /// Initializes a `FinalState` from a snapshot. Currently, we do not use the final_state from the ledger,
    /// we just create a new one. This will be changed in the follow-up.
    ///
    /// # Arguments
    /// * `config`: the configuration of the final state to use for initialization
    /// * `ledger`: the instance of the ledger on disk. Used to apply changes to the ledger.
    /// * `selector`: the pos selector. Used to send draw inputs when a new cycle is completed.
    /// * `last_start_period`: at what period we should attach the final_state
    pub fn new_derived_from_snapshot(
        db: Arc<RwLock<MassaDB>>,
        config: FinalStateConfig,
        ledger: Box<dyn LedgerController>,
        selector: Box<dyn SelectorController>,
        last_start_period: u64,
    ) -> Result<Self, FinalStateError> {
        info!("Restarting from snapshot");

        // FIRST, we recover the last known final_state
        let mut final_state = FinalState::new(db, config, ledger, selector, false)?;
        final_state.pos_state.create_initial_cycle();

        final_state.slot = final_state
            .db
            .read()
            .get_slot(final_state.config.thread_count)
            .map_err(|_| {
                FinalStateError::InvalidSlot(String::from("Could not recover Slot in Ledger"))
            })?;

        debug!(
            "Latest consistent slot found in snapshot data: {}",
            final_state.slot
        );

        info!(
            "final_state hash at slot {}: {}",
            final_state.slot,
            final_state.db.read().get_db_hash()
        );

        // Then, interpolate the downtime, to attach at end_slot;
        final_state.last_start_period = last_start_period;

        final_state.init_ledger_hash(last_start_period);

        // We compute the draws here because we need to feed_cycles when interpolating
        final_state.compute_initial_draws()?;

        final_state.interpolate_downtime()?;

        Ok(final_state)
    }

    /// Used after bootstrap, to set the initial ledger hash (used in initial draws)
    pub fn init_ledger_hash(&mut self, last_start_period: u64) {
        let slot = Slot::new(
            last_start_period,
            self.config.thread_count.saturating_sub(1),
        );
        let mut batch = DBBatch::new(self.db.read().get_db_hash());
        self.db.read().set_slot(slot, &mut batch);
        self.db.read().write_batch(batch);
        self.pos_state.initial_ledger_hash = self.db.read().get_db_hash();

        info!(
            "Set initial ledger hash to {}",
            self.db.read().get_db_hash().to_string()
        )
    }

    /// Once we created a FinalState from a snapshot, we need to edit it to attach at the end_slot and handle the downtime.
    /// This basically recreates the history of the final_state, without executing the slots.
    fn interpolate_downtime(&mut self) -> Result<(), FinalStateError> {
        // TODO: Change the current_slot when we deserialize the final state from RocksDB. Until then, final_state slot and the ledger slot are not consistent!
        // let current_slot = self.slot;
        let current_slot = Slot::new(0, self.config.thread_count.saturating_sub(1));
        let current_slot_cycle = current_slot.get_cycle(self.config.periods_per_cycle);

        let end_slot = Slot::new(
            self.last_start_period,
            self.config.thread_count.saturating_sub(1),
        );
        let end_slot_cycle = end_slot.get_cycle(self.config.periods_per_cycle);

        if current_slot_cycle == end_slot_cycle {
            // In that case, we just complete the gap in the same cycle
            self.interpolate_single_cycle(current_slot, end_slot)?;
        } else {
            // Here, we we also complete the cycle_infos in between
            self.interpolate_multiple_cycles(
                current_slot,
                end_slot,
                current_slot_cycle,
                end_slot_cycle,
            )?;
        }

        self.slot = end_slot;

        // Recompute the hash with the updated data and feed it to POS_state.
        info!(
            "final_state hash at slot {}: {}",
            self.slot,
            self.db.read().get_db_hash()
        );

        // feed final_state_hash to the last cycle
        let cycle = self.slot.get_cycle(self.config.periods_per_cycle);
        self.pos_state
            .feed_cycle_state_hash(cycle, self.final_state_hash);

        Ok(())
    }

    /// This helper function is to be called if the downtime does not span over multiple cycles
    fn interpolate_single_cycle(
        &mut self,
        current_slot: Slot,
        end_slot: Slot,
    ) -> Result<(), FinalStateError> {
        let latest_snapshot_cycle =
            self.pos_state
                .cycle_history_cache
                .pop_back()
                .ok_or(FinalStateError::SnapshotError(String::from(
                    "Invalid cycle_history",
                )))?;

        let latest_snapshot_cycle_info = self.pos_state.get_cycle_info(latest_snapshot_cycle.0);
        self.pos_state.delete_cycle_info(latest_snapshot_cycle.0);

        self.pos_state
            .create_new_cycle_from_last(
                &latest_snapshot_cycle_info,
                current_slot
                    .get_next_slot(self.config.thread_count)
                    .expect("Cannot get next slot"),
                end_slot,
            )
            .map_err(|err| FinalStateError::PosError(format!("{}", err)))?;

        Ok(())
    }

    /// This helper function is to be called if the downtime spans over multiple cycles
    fn interpolate_multiple_cycles(
        &mut self,
        current_slot: Slot,
        end_slot: Slot,
        current_slot_cycle: u64,
        end_slot_cycle: u64,
    ) -> Result<(), FinalStateError> {
        let latest_snapshot_cycle =
            self.pos_state
                .cycle_history_cache
                .pop_back()
                .ok_or(FinalStateError::SnapshotError(String::from(
                    "Invalid cycle_history",
                )))?;

        let latest_snapshot_cycle_info = self.pos_state.get_cycle_info(latest_snapshot_cycle.0);
        self.pos_state.delete_cycle_info(latest_snapshot_cycle.0);

        // Firstly, complete the first cycle
        let last_slot = Slot::new_last_of_cycle(
            current_slot_cycle,
            self.config.periods_per_cycle,
            self.config.thread_count,
        )
        .map_err(|err| {
            FinalStateError::InvalidSlot(format!(
                "Cannot create slot for interpolating downtime: {}",
                err
            ))
        })?;

        self.pos_state
            .create_new_cycle_from_last(
                &latest_snapshot_cycle_info,
                current_slot
                    .get_next_slot(self.config.thread_count)
                    .expect("Cannot get next slot"),
                last_slot,
            )
            .map_err(|err| FinalStateError::PosError(format!("{}", err)))?;

        // Feed final_state_hash to the completed cycle
        self.feed_cycle_hash_and_selector_for_interpolation(current_slot_cycle)?;

        // Then, build all the completed cycles in betweens. If we have to build more cycles than the cycle_history_length, we only build the last ones.
        let current_slot_cycle = (current_slot_cycle + 1)
            .max(end_slot_cycle.saturating_sub(self.config.pos_config.cycle_history_length as u64));

        for cycle in current_slot_cycle..end_slot_cycle {
            let first_slot = Slot::new_first_of_cycle(cycle, self.config.periods_per_cycle)
                .map_err(|err| {
                    FinalStateError::InvalidSlot(format!(
                        "Cannot create slot for interpolating downtime: {}",
                        err
                    ))
                })?;

            let last_slot = Slot::new_last_of_cycle(
                cycle,
                self.config.periods_per_cycle,
                self.config.thread_count,
            )
            .map_err(|err| {
                FinalStateError::InvalidSlot(format!(
                    "Cannot create slot for interpolating downtime: {}",
                    err
                ))
            })?;

            self.pos_state
                .create_new_cycle_from_last(&latest_snapshot_cycle_info, first_slot, last_slot)
                .map_err(|err| FinalStateError::PosError(format!("{}", err)))?;

            // Feed final_state_hash to the completed cycle
            self.feed_cycle_hash_and_selector_for_interpolation(cycle)?;
        }

        // Then, build the last cycle
        let first_slot = Slot::new_first_of_cycle(end_slot_cycle, self.config.periods_per_cycle)
            .map_err(|err| {
                FinalStateError::InvalidSlot(format!(
                    "Cannot create slot for interpolating downtime: {}",
                    err
                ))
            })?;

        self.pos_state
            .create_new_cycle_from_last(&latest_snapshot_cycle_info, first_slot, end_slot)
            .map_err(|err| FinalStateError::PosError(format!("{}", err)))?;

        // If the end_slot_cycle is completed
        if end_slot.is_last_of_cycle(self.config.periods_per_cycle, self.config.thread_count) {
            // Feed final_state_hash to the completed cycle
            self.feed_cycle_hash_and_selector_for_interpolation(end_slot_cycle)?;
        }

        // We reduce the cycle_history len as needed
        while self.pos_state.cycle_history_cache.len() > self.pos_state.config.cycle_history_length
        {
            if let Some((cycle, _)) = self.pos_state.cycle_history_cache.pop_front() {
                self.pos_state.delete_cycle_info(cycle);
            }
        }

        Ok(())
    }

    /// Used during interpolation, when a new cycle is set as completed
    fn feed_cycle_hash_and_selector_for_interpolation(
        &mut self,
        cycle: u64,
    ) -> Result<(), FinalStateError> {
        self.pos_state
            .feed_cycle_state_hash(cycle, self.final_state_hash);

        self.pos_state
            .feed_selector(cycle.checked_add(2).ok_or_else(|| {
                FinalStateError::PosError("cycle overflow when feeding selector".into())
            })?)
            .map_err(|_| {
                FinalStateError::PosError("cycle overflow when feeding selector".into())
            })?;
        Ok(())
    }

    /// Reset the final state to the initial state.
    ///
    /// USED ONLY FOR BOOTSTRAP
    pub fn reset(&mut self) {
        self.slot = Slot::new(0, self.config.thread_count.saturating_sub(1));
        self.ledger.reset();
        self.async_pool.reset();
        self.pos_state.reset();
        self.executed_ops.reset();
        self.executed_denunciations.reset();
        self.changes_history.clear();
        // reset the final state hash
        self.final_state_hash = Hash::from_bytes(FINAL_STATE_HASH_INITIAL_BYTES);
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

        let db = self.db.read();
        let mut db_batch = DBBatch::new(db.get_db_hash());

        // apply the state changes to the batch

        self.async_pool
            .apply_changes_to_batch(&changes.async_pool_changes, &mut db_batch);

        self.pos_state
            .apply_changes_to_batch(changes.pos_changes.clone(), self.slot, true, &mut db_batch)
            .expect("could not settle slot in final state proof-of-stake");
        // TODO:
        // do not panic above, it might just mean that the lookback cycle is not available
        // bootstrap again instead

        self.ledger
            .apply_changes_to_batch(changes.ledger_changes.clone(), &mut db_batch);

        self.executed_ops.apply_changes_to_batch(
            changes.executed_ops_changes.clone(),
            self.slot,
            &mut db_batch,
        );

        self.executed_denunciations.apply_changes_to_batch(
            changes.executed_denunciations_changes.clone(),
            self.slot,
            &mut db_batch,
        );

        self.db.read().set_slot(slot, &mut db_batch);

        self.db.read().write_batch(db_batch);

        // push history element and limit history size
        if self.config.final_history_length > 0 {
            while self.changes_history.len() >= self.config.final_history_length {
                self.changes_history.pop_front();
            }
            self.changes_history.push_back((slot, changes));
        }

        // compute the final state hash
        info!(
            "final_state hash at slot {}: {}",
            self.slot,
            self.db.read().get_db_hash()
        );

        // Backup DB if needed
        if self.slot.period % PERIODS_BETWEEN_BACKUPS == 0 && self.slot.period != 0 {
            let state_slot = self.db.read().get_slot(self.config.thread_count);
            match state_slot {
                Ok(slot) => {
                    info!(
                        "Backuping db for slot {}, state slot: {}, state hash: {}",
                        self.slot,
                        slot,
                        self.db.read().get_db_hash()
                    );
                }
                Err(e) => {
                    info!("{}", e);
                    info!(
                        "Backuping db for unknown state slot, state hash: {}",
                        self.db.read().get_db_hash()
                    );
                }
            }

            self.db.read().backup_db(self.slot);
        }

        // feed final_state_hash to the last cycle
        let cycle = slot.get_cycle(self.config.periods_per_cycle);
        self.pos_state
            .feed_cycle_state_hash(cycle, self.final_state_hash);
    }

    #[allow(clippy::type_complexity)]
    /// Get a part of the state.
    /// Used for bootstrap.
    ///
    /// # Arguments
    /// * cursor: current bootstrap state
    ///
    /// # Returns
    /// The state part and the updated cursor
    pub fn get_state_part(
        &self,
        cursor: StreamingStep<Vec<u8>>,
    ) -> (BTreeMap<Vec<u8>, Vec<u8>>, StreamingStep<Vec<u8>>) {
        let db = self.db.read();
        let handle = db.0.cf_handle(STATE_CF).expect(CF_ERROR);

        let mut state_part = BTreeMap::new();

        // Creates an iterator from the next element after the last if defined, otherwise initialize it at the first key.
        let (db_iterator, mut new_cursor) = match cursor {
            StreamingStep::Started => (
                db.0.iterator_cf(handle, IteratorMode::Start),
                StreamingStep::Started,
            ),
            StreamingStep::Ongoing(last_key) => (
                db.0.iterator_cf(handle, IteratorMode::From(&last_key, Direction::Forward)),
                StreamingStep::Finished(None),
            ),
            StreamingStep::Finished(_) => return (state_part, cursor),
        };

        for (serialized_key, serialized_value) in db_iterator.flatten() {
            while state_part.len() < self.config.ledger_config.max_ledger_part_size as usize {
                state_part.insert(serialized_key.to_vec(), serialized_value.to_vec());
                new_cursor = StreamingStep::Ongoing(serialized_key.to_vec());
            }
        }
        (state_part, new_cursor)
    }

    /// Set a part of the async pool.
    /// Used for bootstrap.
    ///
    /// # Arguments
    /// * part: the async pool part provided by `get_pool_part`
    ///
    /// # Returns
    /// The updated cursor after the current insert
    pub fn set_state_part(&self, part: BTreeMap<Vec<u8>, Vec<u8>>) -> StreamingStep<Vec<u8>> {
        let db = self.db.read();
        let mut batch = DBBatch::new(db.get_db_hash());
        let handle = db.0.cf_handle(STATE_CF).expect(CF_ERROR);

        let cursor = if let Some(key) = part.last_key_value().map(|kv| kv.0.clone()) {
            StreamingStep::Ongoing(key)
        } else {
            StreamingStep::Finished(None)
        };

        for (serialized_key, serialized_value) in part {
            db.put_or_update_entry_value(handle, &mut batch, serialized_key, &serialized_value);
        }

        db.write_batch(batch);

        cursor
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
    #[allow(clippy::too_many_arguments)]
    pub fn get_state_changes_part(
        &self,
        slot: Slot,
        state_step: StreamingStep<Vec<u8>>,
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

            match state_step.clone() {
                StreamingStep::Ongoing(serialized_key) => {
                    if serialized_key.starts_with(LEDGER_PREFIX.as_bytes()) {
                        let (_, key) =
                            KeyDeserializer::new(self.config.ledger_config.max_key_length, true)
                                .deserialize::<DeserializeError>(&serialized_key)
                                .expect(KEY_DESER_ERROR);

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
                    } else if serialized_key.as_slice() >= LEDGER_PREFIX.as_bytes() {
                        slot_changes.ledger_changes = changes.ledger_changes.clone();
                    }

                    if serialized_key.starts_with(ASYNC_POOL_PREFIX.as_bytes()) {
                        let (_, last_id) =
                            AsyncMessageIdDeserializer::new(self.config.thread_count)
                                .deserialize::<DeserializeError>(
                                    &serialized_key[ASYNC_POOL_PREFIX.len()..],
                                )
                                .expect(MESSAGE_ID_DESER_ERROR);

                        let async_pool_changes: AsyncPoolChanges = AsyncPoolChanges(
                            changes
                                .async_pool_changes
                                .0
                                .clone()
                                .into_iter()
                                .filter_map(|change| match change {
                                    (id, _) if id <= last_id => Some(change.clone()),
                                    _ => None,
                                })
                                .collect(),
                        );
                        slot_changes.async_pool_changes = async_pool_changes;
                    } else if serialized_key.as_slice() >= ASYNC_POOL_PREFIX.as_bytes() {
                        slot_changes.async_pool_changes = changes.async_pool_changes.clone();
                    }

                    if serialized_key.starts_with(DEFERRED_CREDITS_PREFIX.as_bytes()) {
                        let (_, cursor_slot) = SlotDeserializer::new(
                            (Included(u64::MIN), Included(u64::MAX)),
                            (Included(0), Excluded(self.config.thread_count)),
                        )
                        .deserialize::<DeserializeError>(
                            &serialized_key[DEFERRED_CREDITS_PREFIX.len()..],
                        )
                        .expect(SLOT_DESER_ERROR);

                        let mut deferred_credits = DeferredCredits::new_with_hash();
                        deferred_credits.credits = changes
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
                            .collect();

                        slot_changes.pos_changes.deferred_credits = deferred_credits;
                    } else if serialized_key.as_slice() >= DEFERRED_CREDITS_PREFIX.as_bytes() {
                        slot_changes.pos_changes.deferred_credits =
                            changes.pos_changes.deferred_credits.clone();
                    }

                    if serialized_key.as_slice() >= CYCLE_HISTORY_PREFIX.as_bytes() {
                        slot_changes.pos_changes.seed_bits = changes.pos_changes.seed_bits.clone();
                        slot_changes.pos_changes.roll_changes =
                            changes.pos_changes.roll_changes.clone();
                        slot_changes.pos_changes.production_stats =
                            changes.pos_changes.production_stats.clone();
                    }
                    if serialized_key.as_slice() >= EXECUTED_OPS_PREFIX.as_bytes() {
                        slot_changes.executed_ops_changes = changes.executed_ops_changes.clone();
                    }

                    if serialized_key.as_slice() >= EXECUTED_DENUNCIATIONS_PREFIX.as_bytes() {
                        slot_changes.executed_denunciations_changes =
                            changes.executed_denunciations_changes.clone();
                    }
                }
                StreamingStep::Finished(_) => {
                    slot_changes = changes.clone();
                }
                _ => (),
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
    use massa_models::{address::Address, config::THREAD_COUNT, slot::Slot};
    use massa_signature::KeyPair;

    fn get_random_address() -> Address {
        let keypair = KeyPair::generate();
        Address::from_public_key(&keypair.get_public_key())
    }

    #[test]
    fn get_state_changes_part() {
        let message = get_random_message(None, THREAD_COUNT);
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
            .insert(message.compute_id(), SetUpdateOrDelete::Set(message));
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
