//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the final state of the node, which includes
//! the final ledger and asynchronous message pool that are kept at
//! the output of a given final slot (the latest executed final slot),
//! and need to be bootstrapped by nodes joining the network.

use crate::{config::FinalStateConfig, error::FinalStateError, state_changes::StateChanges};
use massa_async_pool::{
    AsyncMessage, AsyncMessageId, AsyncPool, AsyncPoolChanges, AsyncPoolDeserializer,
    AsyncPoolSerializer, Change,
};
use massa_executed_ops::{ExecutedOps, ExecutedOpsDeserializer, ExecutedOpsSerializer};
use massa_hash::{Hash, HashDeserializer, HASH_SIZE_BYTES};
use massa_ledger_exports::{Key as LedgerKey, LedgerChanges, LedgerController};
use massa_models::{
    // TODO: uncomment when deserializing the final state from ledger
    /*config::{
        MAX_ASYNC_POOL_LENGTH, MAX_DATASTORE_KEY_LENGTH, MAX_DEFERRED_CREDITS_LENGTH,
        MAX_EXECUTED_OPS_LENGTH, MAX_OPERATIONS_PER_BLOCK, MAX_PRODUCTION_STATS_LENGTH,
        MAX_ROLLS_COUNT_LENGTH,
    },*/
    operation::OperationId,
    prehash::PreHashSet,
    slot::{Slot, SlotDeserializer, SlotSerializer},
    streaming_step::StreamingStep,
};
use massa_pos_exports::{
    CycleHistoryDeserializer, CycleHistorySerializer, CycleInfo, DeferredCredits,
    DeferredCreditsDeserializer, DeferredCreditsSerializer, PoSFinalState, SelectorController,
};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::{error::context, sequence::tuple, IResult, Parser};
use std::collections::{BTreeMap, VecDeque};
use std::ops::Bound::{Excluded, Included};
use tracing::{debug, info};

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
    /// last_start_period
    /// * If start all new network: set to 0
    /// * If from snapshot: retrieve from args
    /// * If from bootstrap: set during bootstrap
    pub last_start_period: u64,
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
            last_start_period: 0,
        })
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
        config: FinalStateConfig,
        ledger: Box<dyn LedgerController>,
        selector: Box<dyn SelectorController>,
        last_start_period: u64,
    ) -> Result<Self, FinalStateError> {
        info!("Restarting from snapshot");

        // FIRST, we recover the last known final_state
        let mut final_state = FinalState::new(config, ledger, selector)?;
        let _final_state_hash_from_snapshot = Hash::from_bytes(FINAL_STATE_HASH_INITIAL_BYTES);
        final_state.pos_state.create_initial_cycle();

        // TODO: We recover the final_state from the RocksDB instance instead
        /*let final_state_data = ledger
        .get_final_state()
        .expect("Cannot retrieve ledger final_state data");*/

        final_state.slot = final_state.ledger.get_slot().map_err(|_| {
            FinalStateError::InvalidSlot(String::from("Could not recover Slot in Ledger"))
        })?;

        debug!(
            "Latest consistent slot found in snapshot data: {}",
            final_state.slot
        );

        //final_state.compute_state_hash_at_slot(final_state.slot);

        // Check the hash to see if we correctly recovered the snapshot
        // TODO: Redo this check when we get the final_state from the ledger
        /*if final_state.final_state_hash != final_state_hash_from_snapshot {
            warn!("The hash of the final_state recovered from the snapshot is different from the hash saved.");
        }*/

        // Then, interpolate the downtime, to attach at end_slot;
        final_state.last_start_period = last_start_period;

        final_state.init_ledger_hash(last_start_period);

        final_state.compute_state_hash_at_slot(final_state.slot);

        final_state.interpolate_downtime()?;

        Ok(final_state)
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
        self.compute_state_hash_at_slot(self.slot);

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
        let latest_snapshot_cycle_info =
            self.pos_state
                .cycle_history
                .back_mut()
                .ok_or(FinalStateError::SnapshotError(String::from(
                    "Invalid cycle_history",
                )))?;

        for _ in current_slot.period..end_slot.period {
            latest_snapshot_cycle_info.rng_seed.push(false);
        }

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
        let latest_snapshot_cycle_info =
            self.pos_state
                .cycle_history
                .back_mut()
                .ok_or(FinalStateError::SnapshotError(String::from(
                    "Invalid cycle_history",
                )))?;

        // This clone lets us repeat the cycle_info for new cycle_infos
        let latest_snapshot_cycle_info_clone = latest_snapshot_cycle_info.clone();

        // Firstly, complete the first cycle
        let latest_cycle_info =
            self.pos_state
                .cycle_history
                .back_mut()
                .ok_or(FinalStateError::SnapshotError(String::from(
                    "Invalid cycle_history",
                )))?;

        for _ in current_slot.period..self.config.periods_per_cycle {
            latest_cycle_info.rng_seed.push(false);
        }

        latest_cycle_info.rng_seed.extend(vec![
            false;
            self.config
                .periods_per_cycle
                .saturating_sub(current_slot.period)
                as usize
        ]);

        latest_cycle_info.complete = true;

        // Feed final_state_hash to the completed cycle
        self.pos_state
            .feed_cycle_state_hash(current_slot_cycle, self.final_state_hash);

        // Then, build all the already completed cycles
        for cycle in (current_slot_cycle + 1)..end_slot_cycle {
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
                .create_new_cycle_from_last(&latest_snapshot_cycle_info_clone, last_slot)
                .map_err(|err| FinalStateError::PosError(format!("{}", err)))?;

            // Feed final_state_hash to the completed cycle
            self.pos_state
                .feed_cycle_state_hash(cycle, self.final_state_hash);
        }

        // Then, build the last cycle
        self.pos_state
            .create_new_cycle_from_last(&latest_snapshot_cycle_info_clone, end_slot)
            .map_err(|err| FinalStateError::PosError(format!("{}", err)))?;

        // If the end_slot_cycle is completed
        if end_slot.is_last_of_cycle(self.config.periods_per_cycle, self.config.thread_count) {
            // Feed final_state_hash to the completed cycle
            self.pos_state
                .feed_cycle_state_hash(end_slot_cycle, self.final_state_hash);
        }

        // We reduce the cycle_history len as needed
        while self.pos_state.cycle_history.len() > self.pos_state.config.cycle_history_length {
            self.pos_state.cycle_history.pop_front();
        }

        Ok(())
    }

    /// Used after bootstrap, to set the initial ledger hash (used in initial draws)
    pub fn init_ledger_hash(&mut self, last_start_period: u64) {
        let slot = Slot::new(
            last_start_period,
            self.config.thread_count.saturating_sub(1),
        );
        self.ledger.set_initial_slot(slot);
        self.pos_state.initial_ledger_hash = self.ledger.get_ledger_hash();

        info!(
            "Set initial ledger hash to {}",
            self.ledger.get_ledger_hash().to_string()
        )
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
        let deferred_credit_hash = match self.pos_state.deferred_credits.get_hash() {
            Some(hash) => hash,
            None => self
                .pos_state
                .deferred_credits
                .enable_hash_tracker_and_compute_hash(),
        };
        hash_concat.extend(deferred_credit_hash.to_bytes());
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

        info!("ledger_hash hash at slot {}: {}", slot, ledger_hash);
        let n = (self.pos_state.cycle_history.len() == self.config.pos_config.cycle_history_length)
            as usize;
        for cycle_info in self.pos_state.cycle_history.iter().skip(n) {
            info!(
                "cycle_global_hash hash at slot {}: {}",
                slot, cycle_info.cycle_global_hash
            );
        }

        info!(
            "cycle_history at slot {}: {}",
            slot,
            self.pos_state.cycle_history.len()
        );
        for cycle_info in self.pos_state.cycle_history.clone() {
            info!(
                "cycle_info at slot {}: [cycle: {}, complete: {}, roll_counts: {:?}]",
                slot, cycle_info.cycle, cycle_info.complete, cycle_info.roll_counts
            );
        }

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

        let mut final_state_data = None;

        if cfg!(feature = "create_snapshot") {
            let /*mut*/ final_state_buffer = Vec::new();

            /*let final_state_raw_serializer = FinalStateRawSerializer::new();

            let final_state_raw = FinalStateRaw {
                async_pool_messages: self.async_pool.messages.clone(),
                cycle_history: self.pos_state.cycle_history.clone(),
                deferred_credits: self.pos_state.deferred_credits.clone(),
                sorted_ops: self.executed_ops.sorted_ops.clone(),
                latest_consistent_slot: self.slot,
                final_state_hash_from_snapshot: self.final_state_hash,
            };

            if final_state_raw_serializer
                .serialize(&final_state_raw, &mut final_state_buffer)
                .is_err()
            {
                debug!("Error while trying to serialize final_state");
            }*/

            final_state_data = Some(final_state_buffer)
        }

        self.ledger
            .apply_changes(changes.ledger_changes.clone(), self.slot, final_state_data);

        // push history element and limit history size
        if self.config.final_history_length > 0 {
            while self.changes_history.len() >= self.config.final_history_length {
                self.changes_history.pop_front();
            }
            self.changes_history.push_back((slot, changes));
        }

        // compute the final state hash
        self.compute_state_hash_at_slot(slot);

        if cfg!(feature = "create_snapshot") {
            let /*mut*/ hash_buffer = Vec::new();

            /*
            let hash_serializer = HashSerializer::new();

            if hash_serializer
                .serialize(&self.final_state_hash, &mut hash_buffer)
                .is_err()
            {
                debug!("Error while trying to serialize final_state_hash");
            }*/

            self.ledger.set_final_state_hash(hash_buffer);
        }

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

/// Serializer for `FinalStateRaw`
pub struct FinalStateRawSerializer {
    async_pool_serializer: AsyncPoolSerializer,
    cycle_history_serializer: CycleHistorySerializer,
    deferred_credits_serializer: DeferredCreditsSerializer,
    executed_ops_serializer: ExecutedOpsSerializer,
    slot_serializer: SlotSerializer,
}

impl Default for FinalStateRawSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl From<FinalState> for FinalStateRaw {
    fn from(value: FinalState) -> Self {
        Self {
            async_pool_messages: value.async_pool.messages,
            cycle_history: value.pos_state.cycle_history,
            deferred_credits: value.pos_state.deferred_credits,
            sorted_ops: value.executed_ops.sorted_ops,
            latest_consistent_slot: value.slot,
            final_state_hash_from_snapshot: value.final_state_hash,
        }
    }
}

impl FinalStateRawSerializer {
    /// Initialize a `FinalStateRaweSerializer`
    pub fn new() -> Self {
        Self {
            async_pool_serializer: AsyncPoolSerializer::new(),
            cycle_history_serializer: CycleHistorySerializer::new(),
            deferred_credits_serializer: DeferredCreditsSerializer::new(),
            executed_ops_serializer: ExecutedOpsSerializer::new(),
            slot_serializer: SlotSerializer::new(),
        }
    }
}

impl Serializer<FinalStateRaw> for FinalStateRawSerializer {
    fn serialize(&self, value: &FinalStateRaw, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // Serialize Async Pool
        self.async_pool_serializer
            .serialize(&value.async_pool_messages, buffer)?;

        // Serialize pos state
        self.cycle_history_serializer
            .serialize(&value.cycle_history, buffer)?;
        self.deferred_credits_serializer
            .serialize(&value.deferred_credits, buffer)?;

        // Serialize Executed Ops
        self.executed_ops_serializer
            .serialize(&value.sorted_ops, buffer)?;

        // Serialize metadata
        self.slot_serializer
            .serialize(&value.latest_consistent_slot, buffer)?;

        // /!\ The final_state_hash has to be serialized separately!

        Ok(())
    }
}

pub struct FinalStateRaw {
    async_pool_messages: BTreeMap<AsyncMessageId, AsyncMessage>,
    cycle_history: VecDeque<CycleInfo>,
    deferred_credits: DeferredCredits,
    sorted_ops: BTreeMap<Slot, PreHashSet<OperationId>>,
    latest_consistent_slot: Slot,
    #[allow(dead_code)]
    final_state_hash_from_snapshot: Hash,
}

/// Deserializer for `FinalStateRaw`
pub struct FinalStateRawDeserializer {
    async_deser: AsyncPoolDeserializer,
    cycle_history_deser: CycleHistoryDeserializer,
    deferred_credits_deser: DeferredCreditsDeserializer,
    executed_ops_deser: ExecutedOpsDeserializer,
    slot_deser: SlotDeserializer,
    hash_deser: HashDeserializer,
}

impl FinalStateRawDeserializer {
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)]
    /// Initialize a `FinalStateRawDeserializer`
    pub fn new(
        config: FinalStateConfig,
        max_async_pool_length: u64,
        max_datastore_key_length: u8,
        max_rolls_length: u64,
        max_production_stats_length: u64,
        max_credit_length: u64,
        max_executed_ops_length: u64,
        max_operations_per_block: u32,
    ) -> Self {
        Self {
            async_deser: AsyncPoolDeserializer::new(
                config.thread_count,
                max_async_pool_length,
                config.async_pool_config.max_async_message_data,
                max_datastore_key_length as u32,
            ),
            cycle_history_deser: CycleHistoryDeserializer::new(
                config.pos_config.cycle_history_length as u64,
                max_rolls_length,
                max_production_stats_length,
            ),
            deferred_credits_deser: DeferredCreditsDeserializer::new(
                config.thread_count,
                max_credit_length,
                true,
            ),
            executed_ops_deser: ExecutedOpsDeserializer::new(
                config.thread_count,
                max_executed_ops_length,
                max_operations_per_block as u64,
            ),
            slot_deser: SlotDeserializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Excluded(config.thread_count)),
            ),
            hash_deser: HashDeserializer::new(),
        }
    }
}

impl Deserializer<FinalStateRaw> for FinalStateRawDeserializer {
    fn deserialize<'a, E: nom::error::ParseError<&'a [u8]> + nom::error::ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], FinalStateRaw, E> {
        context("Failed FinalStateRaw deserialization", |buffer| {
            tuple((
                context("Failed async_pool_messages deserialization", |input| {
                    self.async_deser.deserialize(input)
                }),
                context("Failed cycle_history deserialization", |input| {
                    self.cycle_history_deser.deserialize(input)
                }),
                context("Failed deferred_credits deserialization", |input| {
                    self.deferred_credits_deser.deserialize(input)
                }),
                context("Failed executed_ops deserialization", |input| {
                    self.executed_ops_deser.deserialize(input)
                }),
                context("Failed slot deserialization", |input| {
                    self.slot_deser.deserialize(input)
                }),
                context("Failed hash deserialization", |input| {
                    self.hash_deser.deserialize(input)
                }),
            ))
            .map(
                |(
                    async_pool_messages,
                    cycle_history,
                    deferred_credits,
                    sorted_ops,
                    latest_consistent_slot,
                    final_state_hash_from_snapshot,
                )| FinalStateRaw {
                    async_pool_messages,
                    cycle_history: cycle_history.into(),
                    deferred_credits,
                    sorted_ops,
                    latest_consistent_slot,
                    final_state_hash_from_snapshot,
                },
            )
            .parse(buffer)
        })
        .parse(buffer)
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
