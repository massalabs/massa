//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final state of the node, which includes
//! the final ledger and asynchronous message pool that are kept at
//! the output of a given final slot (the latest executed final slot),
//! and need to be bootstrapped by nodes joining the network.

use crate::controller_trait::FinalStateController;
use crate::{config::FinalStateConfig, error::FinalStateError, state_changes::StateChanges};

use anyhow::{anyhow, Result as AnyResult};
use massa_async_pool::AsyncPool;
use massa_db_exports::{
    DBBatch, MassaIteratorMode, ShareableMassaDBController, ASYNC_POOL_PREFIX,
    CYCLE_HISTORY_PREFIX, DEFERRED_CREDITS_PREFIX, EXECUTED_DENUNCIATIONS_PREFIX,
    EXECUTED_OPS_PREFIX, LEDGER_PREFIX, MIP_STORE_PREFIX, STATE_CF,
};
use massa_db_exports::{EXECUTION_TRAIL_HASH_PREFIX, MIP_STORE_STATS_PREFIX, VERSIONING_CF};
use massa_deferred_calls::DeferredCallRegistry;
use massa_executed_ops::ExecutedDenunciations;
use massa_executed_ops::ExecutedOps;
use massa_hash::Hash;
use massa_ledger_exports::LedgerController;
use massa_ledger_exports::SetOrKeep;
use massa_models::operation::OperationId;
use massa_models::slot::Slot;
use massa_models::timeslots::get_block_slot_timestamp;
use massa_pos_exports::{PoSFinalState, SelectorController};
use massa_versioning::versioning::MipStore;
use tracing::{debug, info, warn};

/// Represents a final state `(ledger, async pool, executed_ops, executed_de and the state of the PoS)`
pub struct FinalState {
    /// execution state configuration
    pub(crate) config: FinalStateConfig,
    /// final ledger associating addresses to their balance, executable bytecode and data
    pub ledger: Box<dyn LedgerController>,
    /// asynchronous pool containing messages sorted by priority and their data
    pub async_pool: AsyncPool,
    /// deferred calls
    pub deferred_call_registry: DeferredCallRegistry,
    /// proof of stake state containing cycle history and deferred credits
    pub pos_state: PoSFinalState,
    /// executed operations
    pub executed_ops: ExecutedOps,
    /// executed denunciations
    pub executed_denunciations: ExecutedDenunciations,
    /// MIP store
    pub mip_store: MipStore,
    /// last_start_period
    /// * If start new network: set to 0
    /// * If from snapshot: retrieve from args
    /// * If from bootstrap: set during bootstrap
    pub last_start_period: u64,
    /// last_slot_before_downtime
    /// * None if start new network
    /// * If from snapshot: retrieve from the slot attached to the snapshot
    /// * If from bootstrap: set during bootstrap
    pub last_slot_before_downtime: Option<Slot>,
    /// the RocksDB instance used to write every final_state struct on disk
    pub db: ShareableMassaDBController,
}

impl FinalState {
    /// Initializes a new `FinalState`
    ///
    /// # Arguments
    /// * `config`: the configuration of the final state to use for initialization
    /// * `ledger`: the instance of the ledger on disk. Used to apply changes to the ledger.
    /// * `selector`: the pos selector. Used to send draw inputs when a new cycle is completed.
    /// * `reset_final_state`: if true, we only keep the ledger, and we reset the other fields of the final state
    pub fn new(
        db: ShareableMassaDBController,
        config: FinalStateConfig,
        ledger: Box<dyn LedgerController>,
        selector: Box<dyn SelectorController>,
        mip_store: MipStore,
        reset_final_state: bool,
    ) -> Result<Self, FinalStateError> {
        let db_slot = db
            .read()
            .get_change_id()
            .map_err(|_| FinalStateError::InvalidSlot(String::from("Could not get slot in db")))?;

        // create the pos state
        let pos_state = PoSFinalState::new(
            config.pos_config.clone(),
            &config.initial_seed_string,
            &config.initial_rolls_path,
            selector,
            db.clone(),
        )
        .map_err(|err| FinalStateError::PosError(format!("PoS final state init error: {}", err)))?;

        // attach at the output of the latest initial final slot, that is the last genesis slot
        let slot = if reset_final_state {
            Slot::new(0, config.thread_count.saturating_sub(1))
        } else {
            db_slot
        };

        // create the async pool
        let async_pool = AsyncPool::new(config.async_pool_config.clone(), db.clone());

        // create a default executed ops
        let executed_ops = ExecutedOps::new(config.executed_ops_config.clone(), db.clone());

        // create a default executed denunciations
        let executed_denunciations =
            ExecutedDenunciations::new(config.executed_denunciations_config.clone(), db.clone());

        let deferred_call_registry = DeferredCallRegistry::new(db.clone());

        let mut final_state = FinalState {
            ledger,
            async_pool,
            deferred_call_registry,
            pos_state,
            config,
            executed_ops,
            executed_denunciations,
            mip_store,
            last_start_period: 0,
            last_slot_before_downtime: None,
            db,
        };

        if reset_final_state {
            final_state.db.read().set_initial_change_id(slot);
            // delete the execution trail hash
            final_state
                .db
                .write()
                .delete_prefix(EXECUTION_TRAIL_HASH_PREFIX, STATE_CF, None);
            final_state.async_pool.reset();
            final_state.pos_state.reset();
            final_state.executed_ops.reset();
            final_state.executed_denunciations.reset();
        }

        info!(
            "final_state hash at slot {}: {}",
            slot,
            final_state.db.read().get_xof_db_hash()
        );

        // create the final state
        Ok(final_state)
    }

    /// Once we created a FinalState from a snapshot, we need to edit it to attach at the end_slot and handle the downtime.
    /// This basically recreates the history of the final_state, without executing the slots.
    fn interpolate_downtime(&mut self) -> Result<(), FinalStateError> {
        let current_slot =
            self.db.read().get_change_id().map_err(|_| {
                FinalStateError::InvalidSlot(String::from("Could not get slot in db"))
            })?;
        let current_slot_cycle = current_slot.get_cycle(self.config.periods_per_cycle);

        let end_slot = Slot::new(
            self.last_start_period,
            self.config.thread_count.saturating_sub(1),
        );
        let end_slot_cycle = end_slot.get_cycle(self.config.periods_per_cycle);

        debug!(
            "Interpolating downtime between slots {} and {}",
            current_slot, end_slot
        );

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

        // Recompute the hash with the updated data and feed it to POS_state.
        let final_state_hash = self.db.read().get_xof_db_hash();

        info!(
            "final_state hash at slot {}: {}",
            end_slot, final_state_hash
        );

        // feed final_state_hash to the last cycle
        let cycle = end_slot.get_cycle(self.config.periods_per_cycle);

        self.pos_state
            .feed_cycle_state_hash(cycle, final_state_hash);

        Ok(())
    }

    /// This helper function is to be called if the downtime does not span over multiple cycles
    fn interpolate_single_cycle(
        &mut self,
        current_slot: Slot,
        end_slot: Slot,
    ) -> Result<(), FinalStateError> {
        let latest_snapshot_cycle = self.pos_state.cycle_history_cache.back().cloned().ok_or(
            FinalStateError::SnapshotError(String::from(
                "Impossible to interpolate the downtime: no cycle in the given snapshot",
            )),
        )?;

        let latest_snapshot_cycle_info = self
            .pos_state
            .get_cycle_info(latest_snapshot_cycle.0)
            .ok_or_else(|| FinalStateError::SnapshotError(String::from("Missing cycle info")))?;

        let mut batch = DBBatch::new();

        self.pos_state
            .cycle_history_cache
            .pop_back()
            .ok_or(FinalStateError::SnapshotError(String::from(
                "Impossible to interpolate the downtime: no cycle in the given snapshot",
            )))?;
        self.pos_state
            .delete_cycle_info(latest_snapshot_cycle.0, &mut batch);

        self.pos_state
            .db
            .write()
            .write_batch(batch, Default::default(), Some(end_slot));

        let mut batch = DBBatch::new();

        self.pos_state
            .create_new_cycle_from_last(
                &latest_snapshot_cycle_info,
                current_slot
                    .get_next_slot(self.config.thread_count)
                    .expect("Cannot get next slot"),
                end_slot,
                &mut batch,
            )
            .map_err(|err| FinalStateError::PosError(format!("{}", err)))?;

        self.pos_state
            .db
            .write()
            .write_batch(batch, Default::default(), Some(end_slot));

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
        let latest_snapshot_cycle = self.pos_state.cycle_history_cache.back().cloned().ok_or(
            FinalStateError::SnapshotError(String::from(
                "Impossible to interpolate the downtime: no cycle in the given snapshot",
            )),
        )?;

        let latest_snapshot_cycle_info = self
            .pos_state
            .get_cycle_info(latest_snapshot_cycle.0)
            .ok_or_else(|| FinalStateError::SnapshotError(String::from("Missing cycle info")))?;

        let mut batch = DBBatch::new();

        self.pos_state
            .cycle_history_cache
            .pop_back()
            .ok_or(FinalStateError::SnapshotError(String::from(
                "Impossible to interpolate the downtime: no cycle in the given snapshot",
            )))?;
        self.pos_state
            .delete_cycle_info(latest_snapshot_cycle.0, &mut batch);

        self.pos_state
            .db
            .write()
            .write_batch(batch, Default::default(), Some(end_slot));

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

        let mut batch = DBBatch::new();

        self.pos_state
            .create_new_cycle_from_last(
                &latest_snapshot_cycle_info,
                current_slot
                    .get_next_slot(self.config.thread_count)
                    .expect("Cannot get next slot"),
                last_slot,
                &mut batch,
            )
            .map_err(|err| FinalStateError::PosError(format!("{}", err)))?;

        self.pos_state
            .db
            .write()
            .write_batch(batch, Default::default(), Some(end_slot));

        // Feed final_state_hash to the completed cycle
        self.feed_cycle_hash_and_selector_for_interpolation(current_slot_cycle)?;

        // TODO: Bring back the following optimisation (it fails because of selector)
        // Then, build all the completed cycles in betweens. If we have to build more cycles than the cycle_history_length, we only build the last ones.
        //let current_slot_cycle = (current_slot_cycle + 1)
        //    .max(end_slot_cycle.saturating_sub(self.config.pos_config.cycle_history_length as u64));
        let current_slot_cycle = current_slot_cycle + 1;

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

            let mut batch = DBBatch::new();

            self.pos_state
                .create_new_cycle_from_last(
                    &latest_snapshot_cycle_info,
                    first_slot,
                    last_slot,
                    &mut batch,
                )
                .map_err(|err| FinalStateError::PosError(format!("{}", err)))?;

            self.pos_state
                .db
                .write()
                .write_batch(batch, Default::default(), Some(end_slot));

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

        let mut batch = DBBatch::new();

        self.pos_state
            .create_new_cycle_from_last(
                &latest_snapshot_cycle_info,
                first_slot,
                end_slot,
                &mut batch,
            )
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
                self.pos_state.delete_cycle_info(cycle, &mut batch);
            }
        }

        self.db
            .write()
            .write_batch(batch, Default::default(), Some(end_slot));

        Ok(())
    }

    /// Used during interpolation, when a new cycle is set as completed
    fn feed_cycle_hash_and_selector_for_interpolation(
        &mut self,
        cycle: u64,
    ) -> Result<(), FinalStateError> {
        let final_state_hash = self.db.read().get_xof_db_hash();

        self.pos_state
            .feed_cycle_state_hash(cycle, final_state_hash);

        self.pos_state
            .feed_selector(cycle.checked_add(2).ok_or_else(|| {
                FinalStateError::PosError("cycle overflow when feeding selector".into())
            })?)
            .map_err(|_| {
                FinalStateError::PosError("cycle overflow when feeding selector".into())
            })?;
        Ok(())
    }

    fn _finalize(&mut self, slot: Slot, changes: StateChanges) -> AnyResult<()> {
        let cur_slot = self.db.read().get_change_id()?;
        // check slot consistency
        let next_slot = cur_slot.get_next_slot(self.config.thread_count)?;

        // .expect("overflow in execution state slot");

        if slot != next_slot {
            return Err(anyhow!(
                "attempting to apply execution state changes at slot {} while the current slot is {}",
                slot, cur_slot
            ));
        }

        let mut db_batch = DBBatch::new();
        let mut db_versioning_batch = DBBatch::new();

        // apply the state changes to the batch

        self.async_pool
            .apply_changes_to_batch(&changes.async_pool_changes, &mut db_batch);
        self.pos_state
            .apply_changes_to_batch(changes.pos_changes, slot, true, &mut db_batch)?;

        // do not panic above, it might just mean that the lookback cycle is not available
        // bootstrap again instead
        self.ledger
            .apply_changes_to_batch(changes.ledger_changes, &mut db_batch);
        self.executed_ops
            .apply_changes_to_batch(changes.executed_ops_changes, slot, &mut db_batch);

        self.deferred_call_registry
            .apply_changes_to_batch(changes.deferred_call_changes, &mut db_batch);

        self.executed_denunciations.apply_changes_to_batch(
            changes.executed_denunciations_changes,
            slot,
            &mut db_batch,
        );

        let slot_ts = get_block_slot_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            slot,
        )?;

        let slot_prev_ts = get_block_slot_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            slot.get_prev_slot(self.config.thread_count)?,
        )?;

        self.mip_store.update_batches(
            &mut db_batch,
            &mut db_versioning_batch,
            Some((&slot_prev_ts, &slot_ts)),
        )?;

        // Update execution trail hash
        if let SetOrKeep::Set(new_hash) = changes.execution_trail_hash_change {
            db_batch.insert(
                EXECUTION_TRAIL_HASH_PREFIX.as_bytes().to_vec(),
                Some(new_hash.to_bytes().to_vec()),
            );
        }

        self.db
            .write()
            .write_batch(db_batch, db_versioning_batch, Some(slot));

        let final_state_hash = self.db.read().get_xof_db_hash();

        // compute the final state hash
        info!("final_state hash at slot {}: {}", slot, final_state_hash);

        // Backup DB if needed
        #[cfg(feature = "bootstrap_server")]
        if slot.period % self.config.ledger_backup_periods_interval == 0
            && slot.period != 0
            && slot.thread == 0
        {
            let state_slot = self.db.read().get_change_id();
            match state_slot {
                Ok(slot) => {
                    info!(
                        "Backuping db for slot {}, state slot: {}, state hash: {}",
                        slot, slot, final_state_hash
                    );
                }
                Err(e) => {
                    info!("{}", e);
                    info!(
                        "Backuping db for unknown state slot, state hash: {}",
                        final_state_hash
                    );
                }
            }

            self.db.read().backup_db(slot);
        }

        // feed final_state_hash to the last cycle
        let cycle = slot.get_cycle(self.config.periods_per_cycle);
        self.pos_state
            .feed_cycle_state_hash(cycle, final_state_hash);

        Ok(())
    }

    /// Internal function called by is_db_valid
    fn _is_db_valid(&self) -> AnyResult<()> {
        let db = self.db.read();

        // check if the execution trial hash is present and valid
        {
            let execution_trail_hash_serialized =
                match db.get_cf(STATE_CF, EXECUTION_TRAIL_HASH_PREFIX.as_bytes().to_vec()) {
                    Ok(Some(v)) => v,
                    Ok(None) => {
                        return Err(anyhow!("No execution trail hash found in DB"));
                    }
                    Err(err) => {
                        return Err(err.into());
                    }
                };
            if let Err(err) = massa_hash::Hash::try_from(&execution_trail_hash_serialized[..]) {
                warn!("Invalid execution trail hash found in DB: {}", err);
                return Err(err.into());
            }
        }

        for (serialized_key, serialized_value) in db.iterator_cf(STATE_CF, MassaIteratorMode::Start)
        {
            #[allow(clippy::if_same_then_else)]
            if serialized_key.starts_with(CYCLE_HISTORY_PREFIX.as_bytes()) {
                if !self
                    .pos_state
                    .is_cycle_history_key_value_valid(&serialized_key, &serialized_value)
                {
                    warn!(
                        "Wrong key/value for CYCLE_HISTORY_KEY PREFIX serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    );
                    return Err(anyhow!(
                        "Wrong key/value for CYCLE_HISTORY_KEY PREFIX serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    ));
                }
            } else if serialized_key.starts_with(DEFERRED_CREDITS_PREFIX.as_bytes()) {
                if !self
                    .pos_state
                    .is_deferred_credits_key_value_valid(&serialized_key, &serialized_value)
                {
                    warn!(
                        "Wrong key/value for DEFERRED_CREDITS PREFIX serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    );
                    return Err(anyhow!(
                        "Wrong key/value for DEFERRED_CREDITS PREFIX serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    ));
                }
            } else if serialized_key.starts_with(ASYNC_POOL_PREFIX.as_bytes()) {
                if !self
                    .async_pool
                    .is_key_value_valid(&serialized_key, &serialized_value)
                {
                    warn!(
                        "Wrong key/value for ASYNC_POOL PREFIX serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    );
                    return Err(anyhow!(
                        "Wrong key/value for ASYNC_POOL PREFIX serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    ));
                }
            } else if serialized_key.starts_with(EXECUTED_OPS_PREFIX.as_bytes()) {
                if !self
                    .executed_ops
                    .is_key_value_valid(&serialized_key, &serialized_value)
                {
                    warn!(
                        "Wrong key/value for EXECUTED_OPS PREFIX serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    );
                    return Err(anyhow!(
                        "Wrong key/value for EXECUTED_OPS PREFIX serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    ));
                }
            } else if serialized_key.starts_with(EXECUTED_DENUNCIATIONS_PREFIX.as_bytes()) {
                if !self
                    .executed_denunciations
                    .is_key_value_valid(&serialized_key, &serialized_value)
                {
                    warn!("Wrong key/value for EXECUTED_DENUNCIATIONS PREFIX serialized_key: {:?}, serialized_value: {:?}", serialized_key, serialized_value);
                    return Err(anyhow!(
                        "Wrong key/value for EXECUTED_DENUNCIATIONS PREFIX serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    ));
                }
            } else if serialized_key.starts_with(LEDGER_PREFIX.as_bytes()) {
                if !self
                    .ledger
                    .is_key_value_valid(&serialized_key, &serialized_value)
                {
                    warn!("Wrong key/value for LEDGER PREFIX serialized_key: {:?}, serialized_value: {:?}", serialized_key, serialized_value);
                    return Err(anyhow!(
                        "Wrong key/value for LEDGER PREFIX serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    ));
                }
            } else if serialized_key.starts_with(MIP_STORE_PREFIX.as_bytes()) {
                if !self
                    .mip_store
                    .is_key_value_valid(&serialized_key, &serialized_value)
                {
                    warn!("Wrong key/value for MIP Store");
                    return Err(anyhow!(
                        "Wrong key/value for MIP Store serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    ));
                }
            } else if serialized_key.starts_with(EXECUTION_TRAIL_HASH_PREFIX.as_bytes()) {
                // no checks here as they are performed above by direct reading
            } else {
                warn!(
                    "Key/value does not correspond to any prefix: serialized_key: {:?}, serialized_value: {:?}",
                    serialized_key, serialized_value
                );
                return Err(anyhow!(
                    "Key/value does not correspond to any prefix: serialized_key: {:?}, serialized_value: {:?}",
                    serialized_key, serialized_value
                ));
            }
        }

        for (serialized_key, serialized_value) in
            db.iterator_cf(VERSIONING_CF, MassaIteratorMode::Start)
        {
            if serialized_key.starts_with(MIP_STORE_PREFIX.as_bytes())
                || serialized_key.starts_with(MIP_STORE_STATS_PREFIX.as_bytes())
            {
                if !self
                    .mip_store
                    .is_key_value_valid(&serialized_key, &serialized_value)
                {
                    warn!("Wrong key/value for MIP Store");
                    return Err(anyhow!(
                        "Wrong key/value for MIP Store serialized_key: {:?}, serialized_value: {:?}",
                        serialized_key, serialized_value
                    ));
                }
            } else {
                warn!(
                    "Key/value does not correspond to any prefix: serialized_key: {:?}, serialized_value: {:?}",
                    serialized_key, serialized_value
                );
                return Err(anyhow!(
                    "Key/value does not correspond to any prefix: serialized_key: {:?}, serialized_value: {:?}",
                    serialized_key, serialized_value
                ));
            }
        }

        Ok(())
    }

    /// Initializes a `FinalState` from a snapshot.
    ///
    /// # Arguments
    /// * `db`: A type that implements the `MassaDBController` trait. Used to read and write to the database.
    /// * `config`: the configuration of the final state to use for initialization
    /// * `ledger`: the instance of the ledger on disk. Used to apply changes to the ledger.
    /// * `selector`: the pos selector. Used to send draw inputs when a new cycle is completed.
    /// * `last_start_period`: at what period we should attach the final_state
    pub fn new_derived_from_snapshot(
        db: ShareableMassaDBController,
        config: FinalStateConfig,
        ledger: Box<dyn LedgerController>,
        selector: Box<dyn SelectorController>,
        mip_store: MipStore,
        last_start_period: u64,
    ) -> Result<Self, FinalStateError> {
        info!("Restarting from snapshot");

        let mut final_state =
            FinalState::new(db, config.clone(), ledger, selector, mip_store, false)?;

        let recovered_slot =
            final_state.db.read().get_change_id().map_err(|_| {
                FinalStateError::InvalidSlot(String::from("Could not get slot in db"))
            })?;

        // This is needed for `test_bootstrap_server` to work
        if cfg!(feature = "test-exports") {
            let mut batch = DBBatch::new();
            final_state.pos_state.create_initial_cycle(&mut batch);
            final_state
                .db
                .write()
                .write_batch(batch, Default::default(), Some(recovered_slot));
        }

        final_state.last_slot_before_downtime = Some(recovered_slot);

        // Check that MIP store is consistent with the network shutdown time range
        // Assume that the final state has been edited during network shutdown
        let shutdown_start = recovered_slot
            .get_next_slot(config.thread_count)
            .map_err(|e| {
                FinalStateError::InvalidSlot(format!(
                    "Unable to get next slot from recovered slot: {:?}",
                    e
                ))
            })?;
        let shutdown_end = Slot::new(last_start_period, 0)
            .get_prev_slot(config.thread_count)
            .map_err(|e| {
                FinalStateError::InvalidSlot(format!(
                    "Unable to compute prev slot from last start period: {:?}",
                    e
                ))
            })?;
        debug!(
            "Checking if MIP store is consistent against shutdown period: {} - {}",
            shutdown_start, shutdown_end
        );

        final_state
            .mip_store
            .is_consistent_with_shutdown_period(
                shutdown_start,
                shutdown_end,
                config.thread_count,
                config.t0,
                config.genesis_timestamp,
            )
            .map_err(FinalStateError::from)?;

        debug!(
            "Latest consistent slot found in snapshot data: {}",
            recovered_slot
        );

        info!(
            "final_state hash at slot {}: {}",
            recovered_slot,
            final_state.db.read().get_xof_db_hash()
        );

        // Then, interpolate the downtime, to attach at end_slot;
        final_state.last_start_period = last_start_period;

        final_state.recompute_caches();

        // We compute the draws here because we need to feed_cycles when interpolating
        final_state.compute_initial_draws()?;

        final_state.interpolate_downtime()?;

        Ok(final_state)
    }
}

impl FinalStateController for FinalState {
    fn compute_initial_draws(&mut self) -> Result<(), FinalStateError> {
        self.pos_state
            .compute_initial_draws()
            .map_err(|err| FinalStateError::PosError(err.to_string()))
    }

    fn finalize(&mut self, slot: Slot, changes: StateChanges) {
        self._finalize(slot, changes).unwrap()
    }

    fn get_execution_trail_hash(&self) -> Hash {
        let hash_bytes = self
            .db
            .read()
            .get_cf(STATE_CF, EXECUTION_TRAIL_HASH_PREFIX.as_bytes().to_vec())
            .expect("could not read execution trail hash from state DB")
            .expect("could not find execution trail hash in state DB");
        Hash::from_bytes(
            hash_bytes
                .as_slice()
                .try_into()
                .expect("invalid execution trail hash in state DB"),
        )
    }

    fn get_fingerprint(&self) -> Hash {
        let internal_hash = self.db.read().get_xof_db_hash();
        Hash::compute_from(internal_hash.to_bytes())
    }

    fn get_slot(&self) -> Slot {
        self.db
            .read()
            .get_change_id()
            .expect("Critical error: Final state has no slot attached")
    }

    fn init_execution_trail_hash_to_batch(&mut self, batch: &mut DBBatch) {
        batch.insert(
            EXECUTION_TRAIL_HASH_PREFIX.as_bytes().to_vec(),
            Some(massa_hash::Hash::zero().to_bytes().to_vec()),
        );
    }

    fn is_db_valid(&self) -> bool {
        self._is_db_valid().is_ok()
    }

    fn recompute_caches(&mut self) {
        self.async_pool.recompute_message_info_cache();
        self.executed_ops.recompute_sorted_ops_and_op_exec_status();
        self.executed_denunciations.recompute_sorted_denunciations();
        self.pos_state.recompute_pos_state_caches();
    }

    fn reset(&mut self) {
        let slot = Slot::new(0, self.config.thread_count.saturating_sub(1));
        self.db.write().reset(slot);
        self.ledger.reset();
        self.async_pool.reset();
        self.pos_state.reset();
        self.executed_ops.reset();
        self.executed_denunciations.reset();
        self.mip_store.reset_db(self.db.clone());
        // delete the execution trail hash
        self.db
            .write()
            .delete_prefix(EXECUTION_TRAIL_HASH_PREFIX, STATE_CF, None);
    }

    fn get_ledger(&self) -> &Box<dyn LedgerController> {
        &self.ledger
    }

    fn get_ledger_mut(&mut self) -> &mut Box<dyn LedgerController> {
        &mut self.ledger
    }

    fn get_async_pool(&self) -> &AsyncPool {
        &self.async_pool
    }

    fn get_pos_state(&self) -> &PoSFinalState {
        &self.pos_state
    }

    fn get_pos_state_mut(&mut self) -> &mut PoSFinalState {
        &mut self.pos_state
    }

    fn executed_ops_contains(&self, op_id: &OperationId) -> bool {
        self.executed_ops.contains(op_id)
    }

    fn get_ops_exec_status(&self, batch: &[OperationId]) -> Vec<Option<bool>> {
        self.executed_ops.get_ops_exec_status(batch)
    }

    fn get_executed_denunciations(&self) -> &ExecutedDenunciations {
        &self.executed_denunciations
    }

    fn get_database(&self) -> &ShareableMassaDBController {
        &self.db
    }

    fn get_last_start_period(&self) -> u64 {
        self.last_start_period
    }

    fn set_last_start_period(&mut self, last_start_period: u64) {
        self.last_start_period = last_start_period;
    }

    fn get_last_slot_before_downtime(&self) -> &Option<Slot> {
        &self.last_slot_before_downtime
    }

    fn set_last_slot_before_downtime(&mut self, last_slot_before_downtime: Option<Slot>) {
        self.last_slot_before_downtime = last_slot_before_downtime;
    }

    fn get_mip_store_mut(&mut self) -> &mut MipStore {
        &mut self.mip_store
    }

    fn get_mip_store(&self) -> &MipStore {
        &self.mip_store
    }

    fn get_deferred_call_registry(&self) -> &DeferredCallRegistry {
        &self.deferred_call_registry
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::str::FromStr;
    use std::sync::Arc;

    use num::rational::Ratio;
    use parking_lot::RwLock;
    use tempfile::tempdir;

    use massa_async_pool::{AsyncMessage, AsyncPoolChanges, AsyncPoolConfig};
    use massa_db_exports::{MassaDBConfig, MassaDBController, STATE_HASH_INITIAL_BYTES};
    use massa_db_worker::MassaDB;
    use massa_executed_ops::{ExecutedDenunciationsConfig, ExecutedOpsConfig};
    use massa_hash::Hash;
    use massa_ledger_exports::{LedgerChanges, LedgerConfig, LedgerEntryUpdate, SetUpdateOrDelete};
    use massa_ledger_worker::FinalLedger;
    use massa_models::address::Address;
    use massa_models::amount::Amount;
    use massa_models::bytecode::Bytecode;

    use massa_models::config::{
        DENUNCIATION_EXPIRE_PERIODS, ENDORSEMENT_COUNT, KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
        MAX_ASYNC_POOL_LENGTH, MAX_DATASTORE_KEY_LENGTH, MAX_DATASTORE_VALUE_LENGTH,
        MAX_DEFERRED_CREDITS_LENGTH, MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        MAX_DENUNCIATION_CHANGES_LENGTH, MAX_FUNCTION_NAME_LENGTH, MAX_PARAMETERS_SIZE,
        MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH, MIP_STORE_STATS_BLOCK_CONSIDERED,
        PERIODS_PER_CYCLE, POS_SAVED_CYCLES, T0, THREAD_COUNT,
    };
    use massa_pos_exports::MockSelectorController;
    use massa_pos_exports::{PoSChanges, PoSConfig, PosError};
    use massa_time::MassaTime;
    use massa_versioning::versioning::MipStatsConfig;

    use super::*;

    fn get_final_state_config() -> (FinalStateConfig, LedgerConfig) {
        let massa_node_base = PathBuf::from("../massa-node");

        let genesis_timestamp = MassaTime::from_millis(0);
        let ledger_config = LedgerConfig {
            thread_count: THREAD_COUNT,
            initial_ledger_path: massa_node_base.join("base_config/initial_ledger.json"),
            max_key_length: MAX_DATASTORE_KEY_LENGTH,
            max_datastore_value_length: MAX_DATASTORE_VALUE_LENGTH,
        };
        let async_pool_config = AsyncPoolConfig {
            max_length: MAX_ASYNC_POOL_LENGTH,
            max_function_length: MAX_FUNCTION_NAME_LENGTH,
            max_function_params_length: MAX_PARAMETERS_SIZE as u64,
            thread_count: THREAD_COUNT,
            max_key_length: MAX_DATASTORE_KEY_LENGTH as u32,
        };
        let pos_config = PoSConfig {
            periods_per_cycle: PERIODS_PER_CYCLE,
            thread_count: THREAD_COUNT,
            cycle_history_length: POS_SAVED_CYCLES,
            max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
            max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
            max_credit_length: MAX_DEFERRED_CREDITS_LENGTH,
            initial_deferred_credits_path: Some(
                massa_node_base.join("base_config/deferred_credits.json"),
            ),
        };
        let executed_ops_config = ExecutedOpsConfig {
            thread_count: THREAD_COUNT,
            keep_executed_history_extra_periods: KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
        };
        let executed_denunciations_config = ExecutedDenunciationsConfig {
            denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
            thread_count: THREAD_COUNT,
            endorsement_count: ENDORSEMENT_COUNT,
            keep_executed_history_extra_periods: KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
        };

        let final_state_config = FinalStateConfig {
            ledger_config: ledger_config.clone(),
            async_pool_config,
            pos_config,
            executed_ops_config,
            executed_denunciations_config,
            final_history_length: 100, // SETTINGS.ledger.final_history_length,
            thread_count: THREAD_COUNT,
            periods_per_cycle: PERIODS_PER_CYCLE,
            initial_seed_string: "test".to_string(),
            initial_rolls_path: massa_node_base.join("base_config/initial_rolls.json"), // SETTINGS.selector.initial_rolls_path.clone(),
            endorsement_count: ENDORSEMENT_COUNT,
            max_executed_denunciations_length: MAX_DENUNCIATION_CHANGES_LENGTH,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            t0: T0,
            ledger_backup_periods_interval: 10,
            genesis_timestamp,
        };

        (final_state_config, ledger_config)
    }

    fn get_final_state() -> FinalState {
        let (final_state_config, ledger_config) = get_final_state_config();

        let temp_dir_db = tempdir().expect("Unable to create a temp folder");
        // println!("Using temp dir: {:?}", temp_dir.path());

        let db_config = MassaDBConfig {
            path: temp_dir_db.path().to_path_buf(),
            max_history_length: 100,
            max_final_state_elements_size: 100,
            max_versioning_elements_size: 100,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };
        let db = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
        ));

        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100), // In config.toml,
        };
        let mip_store =
            MipStore::try_from(([], mip_stats_config)).expect("Cannot create an empty MIP store");

        let selector_controller = Box::new(MockSelectorController::new());
        let ledger = FinalLedger::new(ledger_config, db.clone());

        FinalState::new(
            db,
            final_state_config,
            Box::new(ledger),
            selector_controller,
            mip_store,
            false,
        )
        .expect("Cannot init final state")
    }

    fn get_state_changes() -> StateChanges {
        let mut state_changes = StateChanges::default();
        let message = AsyncMessage::new(
            Slot::new(1, 0),
            0,
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
            String::from("test"),
            10000000,
            Amount::from_str("1").unwrap(),
            Amount::from_str("1").unwrap(),
            Slot::new(2, 0),
            Slot::new(3, 0),
            vec![1, 2, 3, 4],
            None,
            None,
        );
        let mut async_pool_changes = AsyncPoolChanges::default();
        async_pool_changes
            .0
            .insert(message.compute_id(), SetUpdateOrDelete::Set(message));
        state_changes.async_pool_changes = async_pool_changes;

        let amount = Amount::from_str("1").unwrap();
        let bytecode = Bytecode(vec![1, 2, 3]);
        let ledger_entry = LedgerEntryUpdate {
            balance: SetOrKeep::Set(amount),
            bytecode: SetOrKeep::Set(bytecode),
            datastore: BTreeMap::default(),
        };
        let mut ledger_changes = LedgerChanges::default();
        ledger_changes.0.insert(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            SetUpdateOrDelete::Update(ledger_entry),
        );
        state_changes.ledger_changes = ledger_changes;

        let mut pos_changes = PoSChanges::default();
        pos_changes.roll_changes.insert(
            Address::from_str("AU12r1iM79EcS3sa4dmtUp28TiaPxK1weQcLsATcFoynPdukjdMqM").unwrap(),
            0,
        );
        state_changes.pos_changes = pos_changes;

        state_changes
    }

    #[test]
    fn test_final_state_finalize() {
        // 0- Create a final state
        // 1- Attempt to finalize it with KO data
        // 2- Attempt to finalize it with OK data

        let mut fstate = get_final_state();
        let initial_slot = Slot::new(0, 0);
        assert_eq!(fstate.get_slot(), initial_slot);

        let wrong_next_slot = Slot::new(1, 1);
        let ok_next_slot = Slot::new(0, 1);
        let changes = get_state_changes();

        let res = fstate._finalize(wrong_next_slot, changes.clone());
        assert!(res
            .err()
            .unwrap()
            .to_string()
            .contains("while the current slot is"));

        assert_eq!(fstate.get_slot(), initial_slot);

        // This should also fail because there is no initial cycle (required by POS state)
        let res = fstate._finalize(ok_next_slot, changes.clone());

        assert!(res.is_err());
        match res {
            Ok(_) => unreachable!(),
            Err(e) => match e.downcast_ref() {
                Some(PosError::ContainerInconsistency(_)) => {
                    println!("Received correct error: PosError:ContainerInconsistency...");
                }
                Some(_) => {
                    panic!("Unknown error received: {}", e);
                }
                None => unreachable!(),
            },
        }

        let mut batch = DBBatch::new();
        fstate.pos_state.create_initial_cycle(&mut batch);
        let res = fstate._finalize(ok_next_slot, changes);
        assert!(res.is_ok());
        assert_eq!(fstate.get_slot(), ok_next_slot);
    }

    #[test]
    fn test_final_state_from_snapshot_1() {
        // 0- Create a final state
        // 1- Try to create another final state from snapshot
        //    Restart with slot in the same cycle

        let mut fstate = get_final_state();
        let ok_next_slot = Slot::new(0, 1);
        let changes = get_state_changes();
        let mut batch = DBBatch::new();
        fstate.pos_state.create_initial_cycle(&mut batch);
        let res = fstate._finalize(ok_next_slot, changes);
        assert!(res.is_ok());
        assert_eq!(fstate.get_slot(), ok_next_slot);

        let db_2 = fstate.db.clone();
        let config_2 = fstate.config.clone();
        let ledger_2 = FinalLedger::new(fstate.config.ledger_config.clone(), db_2.clone());
        let mut selector_controller = Box::new(MockSelectorController::new());
        // TODO: more checks
        selector_controller
            .expect_feed_cycle()
            .returning(|_, _, _| Ok(()));
        selector_controller
            .expect_wait_for_draws()
            .returning(|_| Ok(1));
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100), // In config.toml,
        };
        let mip_store =
            MipStore::try_from(([], mip_stats_config)).expect("Cannot create an empty MIP store");

        let last_start_period_2 = 2;
        let fstate2_ = FinalState::new_derived_from_snapshot(
            db_2,
            config_2,
            Box::new(ledger_2),
            selector_controller,
            mip_store,
            last_start_period_2,
        );

        assert!(fstate2_.is_ok());
        let mut fstate2 = fstate2_.unwrap();
        assert_eq!(fstate2.last_slot_before_downtime, Some(ok_next_slot));
        assert_eq!(
            fstate2.get_slot(),
            Slot::new(last_start_period_2, THREAD_COUNT - 1)
        );

        assert!(!fstate2.is_db_valid()); // no trail hash
        fstate2.init_execution_trail_hash_to_batch(&mut batch);
        fstate2
            .db
            .write()
            .write_batch(batch, Default::default(), None);
        assert!(fstate2.is_db_valid());
    }

    #[test]
    fn test_final_state_from_snapshot_2() {
        // 0- Create a final state
        // 1- Try to create another final state from snapshot
        //    Restart with slot with + 2 cycles

        let mut fstate = get_final_state();
        let ok_next_slot = Slot::new(0, 1);
        let changes = get_state_changes();
        let mut batch = DBBatch::new();
        fstate.pos_state.create_initial_cycle(&mut batch);
        let res = fstate._finalize(ok_next_slot, changes);
        assert!(res.is_ok());
        assert_eq!(fstate.get_slot(), ok_next_slot);

        let db_2 = fstate.db.clone();
        let config_2 = fstate.config.clone();
        let ledger_2 = FinalLedger::new(fstate.config.ledger_config.clone(), db_2.clone());
        let mut selector_controller = Box::new(MockSelectorController::new());
        selector_controller
            .expect_feed_cycle()
            .times(4)
            .returning(|_, _, _| Ok(()));
        selector_controller
            .expect_wait_for_draws()
            .returning(|_| Ok(1));
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100), // In config.toml,
        };
        let mip_store =
            MipStore::try_from(([], mip_stats_config)).expect("Cannot create an empty MIP store");

        let last_start_period_2 = 2 + (PERIODS_PER_CYCLE * 2);
        let fstate2_ = FinalState::new_derived_from_snapshot(
            db_2,
            config_2,
            Box::new(ledger_2),
            selector_controller,
            mip_store,
            last_start_period_2,
        );

        assert!(fstate2_.is_ok());
        let mut fstate2 = fstate2_.unwrap();
        assert_eq!(fstate2.last_slot_before_downtime, Some(ok_next_slot));
        assert_eq!(
            fstate2.get_slot(),
            Slot::new(last_start_period_2, THREAD_COUNT - 1)
        );

        assert!(!fstate2.is_db_valid()); // no trail hash
        fstate2.init_execution_trail_hash_to_batch(&mut batch);
        fstate2
            .db
            .write()
            .write_batch(batch, Default::default(), None);
        assert!(fstate2.is_db_valid());
    }

    #[test]
    fn test_final_state_reset() {
        // Create a final state
        // 1- Check valid, fingerprint
        // 2- Init execution trail hash
        // 3- Check valid, fingerprint
        // 4- Reset final state
        // 5- Check valid, fingerprint

        let mut fstate = get_final_state();

        let db_valid = fstate._is_db_valid();
        assert!(db_valid.is_err());
        assert!(db_valid
            .err()
            .unwrap()
            .to_string()
            .starts_with("No execution trail hash"));
        // Check final state fingerprint
        assert_eq!(
            fstate.get_fingerprint(),
            Hash::compute_from(STATE_HASH_INITIAL_BYTES)
        );

        let mut batch = DBBatch::new();
        fstate.init_execution_trail_hash_to_batch(&mut batch);
        fstate
            .db
            .write()
            .write_batch(batch, Default::default(), None);

        let db_valid = fstate._is_db_valid();
        assert!(db_valid.is_ok());

        // Check final state fingerprint before reset
        assert_ne!(
            fstate.get_fingerprint(),
            Hash::compute_from(STATE_HASH_INITIAL_BYTES)
        );

        fstate.reset();

        let db_valid = fstate._is_db_valid();
        // After reset, final_state is not valid anymore
        assert!(db_valid.is_err());
        assert_eq!(fstate.get_slot().period, 0);
        // Check final state fingerprint
        assert_eq!(
            fstate.get_fingerprint(),
            Hash::compute_from(STATE_HASH_INITIAL_BYTES)
        );
    }
}
