use crate::{
    CycleHistoryDeserializer, CycleHistorySerializer, CycleInfo, DeferredCreditsDeserializer,
    DeferredCreditsSerializer, PoSChanges, PosError, PosResult, ProductionStats,
    SelectorController,
};
use crate::{DeferredCredits, PoSConfig};
use bitvec::vec::BitVec;
use massa_db_exports::{
    DBBatch, MassaDirection, MassaIteratorMode, ShareableMassaDBController,
    CYCLE_HISTORY_DESER_ERROR, CYCLE_HISTORY_PREFIX, CYCLE_HISTORY_SER_ERROR,
    DEFERRED_CREDITS_DESER_ERROR, DEFERRED_CREDITS_PREFIX, DEFERRED_CREDITS_SER_ERROR, STATE_CF,
};
use massa_hash::Hash;
use massa_models::amount::Amount;
use massa_models::{address::Address, prehash::PreHashMap, slot::Slot};
use massa_serialization::{DeserializeError, Deserializer, Serializer, U64VarIntSerializer};
use nom::AsBytes;
use std::collections::VecDeque;
use std::ops::Bound::{Excluded, Included};
use std::ops::RangeBounds;
use std::{collections::BTreeMap, path::PathBuf};
use tracing::debug;

// General cycle info idents
const COMPLETE_IDENT: u8 = 0u8;
const RNG_SEED_IDENT: u8 = 1u8;
const FINAL_STATE_HASH_SNAPSHOT_IDENT: u8 = 2u8;
const ROLL_COUNT_IDENT: u8 = 3u8;
const PROD_STATS_IDENT: u8 = 4u8;

// Production stats idents
const PROD_STATS_FAIL_IDENT: u8 = 0u8;
const PROD_STATS_SUCCESS_IDENT: u8 = 1u8;

/// Complete key formatting macro
#[macro_export]
macro_rules! complete_key {
    ($cycle_prefix:expr) => {
        [&$cycle_prefix[..], &[COMPLETE_IDENT]].concat()
    };
}

/// Rng seed key formatting macro
#[macro_export]
macro_rules! rng_seed_key {
    ($cycle_prefix:expr) => {
        [&$cycle_prefix[..], &[RNG_SEED_IDENT]].concat()
    };
}

/// Final state hash snapshot key formatting macro
#[macro_export]
macro_rules! final_state_hash_snapshot_key {
    ($cycle_prefix:expr) => {
        [&$cycle_prefix[..], &[FINAL_STATE_HASH_SNAPSHOT_IDENT]].concat()
    };
}

/// Roll count key prefix macro
#[macro_export]
macro_rules! roll_count_prefix {
    ($cycle_prefix:expr) => {
        [&$cycle_prefix[..], &[ROLL_COUNT_IDENT]].concat()
    };
}

/// Roll count key formatting macro
#[macro_export]
macro_rules! roll_count_key {
    ($cycle_prefix:expr, $addr:expr) => {
        [
            &$cycle_prefix[..],
            &[ROLL_COUNT_IDENT],
            &$addr.to_prefixed_bytes()[..],
        ]
        .concat()
    };
}

/// Production stats prefix macro
#[macro_export]
macro_rules! prod_stats_prefix {
    ($cycle_prefix:expr) => {
        [&$cycle_prefix[..], &[PROD_STATS_IDENT]].concat()
    };
}

/// Production stats fail key formatting macro
#[macro_export]
macro_rules! prod_stats_fail_key {
    ($cycle_prefix:expr, $addr:expr) => {
        [
            &$cycle_prefix[..],
            &[PROD_STATS_IDENT],
            &$addr.to_prefixed_bytes()[..],
            &[PROD_STATS_FAIL_IDENT],
        ]
        .concat()
    };
}

/// Production stats success key formatting macro
#[macro_export]
macro_rules! prod_stats_success_key {
    ($cycle_prefix:expr, $addr:expr) => {
        [
            &$cycle_prefix[..],
            &[PROD_STATS_IDENT],
            &$addr.to_prefixed_bytes()[..],
            &[PROD_STATS_SUCCESS_IDENT],
        ]
        .concat()
    };
}

/// Deferred credits key formatting macro
#[macro_export]
macro_rules! deferred_credits_key {
    ($id:expr) => {
        [&DEFERRED_CREDITS_PREFIX.as_bytes(), &$id[..]].concat()
    };
}

#[derive(Clone)]
/// Final state of PoS
pub struct PoSFinalState {
    /// proof-of-stake configuration
    pub config: PoSConfig,
    /// Access to the RocksDB database
    pub db: ShareableMassaDBController,
    /// contiguous cycle history, back = newest
    pub cycle_history_cache: VecDeque<(u64, bool)>,
    /// rng_seed cache to get rng_seed for the current cycle
    pub rng_seed_cache: Option<(u64, BitVec<u8>)>,
    /// selector controller
    pub selector: Box<dyn SelectorController>,
    /// initial rolls, used for negative cycle look back
    pub initial_rolls: BTreeMap<Address, u64>,
    /// initial seeds, used for negative cycle look back (cycles -2, -1 in that order)
    pub initial_seeds: Vec<Hash>,
    /// deferred credits serializer
    pub deferred_credits_serializer: DeferredCreditsSerializer,
    /// deferred credits deserializer
    pub deferred_credits_deserializer: DeferredCreditsDeserializer,
    /// cycle info serializer
    pub cycle_info_serializer: CycleHistorySerializer,
    /// cycle info deserializer
    pub cycle_info_deserializer: CycleHistoryDeserializer,
}

impl PoSFinalState {
    /// create a new `PoSFinalState`
    pub fn new(
        config: PoSConfig,
        initial_seed_string: &str,
        initial_rolls_path: &PathBuf,
        selector: Box<dyn SelectorController>,
        db: ShareableMassaDBController,
    ) -> Result<Self, PosError> {
        // load get initial rolls from file
        let initial_rolls = serde_json::from_str::<BTreeMap<Address, u64>>(
            &std::fs::read_to_string(initial_rolls_path).map_err(|err| {
                PosError::RollsFileLoadingError(format!("error while deserializing: {}", err))
            })?,
        )
        .map_err(|err| PosError::RollsFileLoadingError(format!("error opening file: {}", err)))?;

        // Seeds used as the initial seeds for negative cycles (-2 and -1 respectively)
        let init_seed = Hash::compute_from(initial_seed_string.as_bytes());
        let initial_seeds = vec![Hash::compute_from(init_seed.to_bytes()), init_seed];

        let deferred_credits_deserializer =
            DeferredCreditsDeserializer::new(config.thread_count, config.max_credit_length, true);
        let cycle_info_deserializer = CycleHistoryDeserializer::new(
            config.cycle_history_length as u64,
            config.max_rolls_length,
            config.max_production_stats_length,
        );

        let pos_state = Self {
            config,
            db,
            cycle_history_cache: Default::default(),
            rng_seed_cache: None,
            selector,
            initial_rolls,
            initial_seeds,
            deferred_credits_serializer: DeferredCreditsSerializer::new(),
            deferred_credits_deserializer,
            cycle_info_serializer: CycleHistorySerializer::new(),
            cycle_info_deserializer,
        };

        Ok(pos_state)
    }

    /// After bootstrap or load from disk, recompute the caches
    pub fn recompute_pos_state_caches(&mut self) {
        self.cycle_history_cache = self.get_cycle_history_cycles().into();

        if let Some((cycle, _)) = self.cycle_history_cache.back() {
            self.rng_seed_cache = Some((*cycle, self.get_cycle_history_rng_seed(*cycle)));
        } else {
            self.rng_seed_cache = None;
        }
    }

    /// Reset the state of the PoS final state
    ///
    /// USED ONLY FOR BOOTSTRAP
    pub fn reset(&mut self) {
        let mut db = self.db.write();
        db.delete_prefix(CYCLE_HISTORY_PREFIX, STATE_CF, None);
        db.delete_prefix(DEFERRED_CREDITS_PREFIX, STATE_CF, None);
        self.cycle_history_cache = Default::default();
        self.rng_seed_cache = None;
    }

    /// Create the initial cycle based off the initial rolls.
    ///
    /// This should be called only if bootstrap did not happen.
    pub fn create_initial_cycle(&mut self, batch: &mut DBBatch) {
        let mut rng_seed = BitVec::with_capacity(
            self.config
                .periods_per_cycle
                .saturating_mul(self.config.thread_count as u64)
                .try_into()
                .unwrap(),
        );
        rng_seed.extend(vec![false; self.config.thread_count as usize]);

        self.put_new_cycle_info(
            &CycleInfo::new_with_hash(
                0,
                false,
                self.initial_rolls.clone(),
                rng_seed,
                PreHashMap::default(),
            ),
            batch,
        );
    }

    /// Create the a cycle based off of another cycle_info.
    ///
    /// Used for downtime interpolation, when restarting from a snapshot.
    pub fn create_new_cycle_from_last(
        &mut self,
        last_cycle_info: &CycleInfo,
        first_slot: Slot,
        last_slot: Slot,
        batch: &mut DBBatch,
    ) -> Result<(), PosError> {
        let mut rng_seed = if first_slot.is_first_of_cycle(self.config.periods_per_cycle) {
            BitVec::with_capacity(
                self.config
                    .periods_per_cycle
                    .saturating_mul(self.config.thread_count as u64)
                    .try_into()
                    .unwrap(),
            )
        } else {
            last_cycle_info.rng_seed.clone()
        };

        let cycle = last_slot.get_cycle(self.config.periods_per_cycle);

        let num_slots = last_slot
            .slots_since(&first_slot, self.config.thread_count)
            .expect("Error in slot ordering")
            .saturating_add(1);

        rng_seed.extend(vec![false; num_slots as usize]);

        let complete =
            last_slot.is_last_of_cycle(self.config.periods_per_cycle, self.config.thread_count);

        self.put_new_cycle_info(
            &CycleInfo::new_with_hash(
                cycle,
                complete,
                last_cycle_info.roll_counts.clone(),
                rng_seed,
                last_cycle_info.production_stats.clone(),
            ),
            batch,
        );

        Ok(())
    }

    /// Deletes a given cycle from RocksDB
    pub fn delete_cycle_info(&mut self, cycle: u64, batch: &mut DBBatch) {
        let db = self.db.read();

        let prefix = self.cycle_history_cycle_prefix(cycle);

        for (serialized_key, _) in db.prefix_iterator_cf(STATE_CF, &prefix) {
            if !serialized_key.starts_with(prefix.as_bytes()) {
                break;
            }
            db.delete_key(batch, serialized_key.to_vec());
        }
    }

    /// Sends the current draw inputs (initial or bootstrapped) to the selector.
    /// Waits for the initial draws to be performed.
    pub fn compute_initial_draws(&mut self) -> PosResult<()> {
        // if cycle_history starts at a cycle that is strictly higher than 0, do not feed cycles 0, 1 to selector
        let history_starts_late = self
            .cycle_history_cache
            .front()
            .map(|c_info| c_info.0 > 0)
            .unwrap_or(false);

        let mut max_cycle = None;

        // feed cycles 0, 1 to selector if necessary
        if !history_starts_late {
            for draw_cycle in 0u64..=1 {
                self.feed_selector(draw_cycle)?;
                max_cycle = Some(draw_cycle);
            }
        }

        // feed cycles available from history
        for (idx, hist_item) in self.cycle_history_cache.iter().enumerate() {
            if !hist_item.1 {
                break;
            }
            if history_starts_late && idx == 0 {
                // If the history starts late, the first RNG seed cannot be used to draw
                // because the roll distribution which should be provided by the previous element is absent.
                continue;
            }
            let draw_cycle = hist_item.0.checked_add(2).ok_or_else(|| {
                PosError::OverflowError("cycle overflow in give_selector_controller".into())
            })?;
            self.feed_selector(draw_cycle)?;
            max_cycle = Some(draw_cycle);
        }

        // wait for all fed cycles to be drawn
        if let Some(wait_cycle) = max_cycle {
            self.selector.as_mut().wait_for_draws(wait_cycle)?;
        }
        Ok(())
    }

    /// Technical specification of `apply_changes_to_batch`:
    ///
    /// set `self.last_final_slot` = C
    /// if cycle C is absent from `self.cycle_history_cache`:
    ///     `push` a new empty `CycleInfo` on disk and reflect in `self.cycle_history_cache` and set its cycle = C
    ///     `pop_front` from `cycle_history_cache` until front() represents cycle C-4 or later (not C-3 because we might need older endorsement draws on the limit between 2 cycles)
    ///     delete the removed cycles from disk
    /// for the cycle C entry in the db:
    ///     extend `seed_bits` with `changes.seed_bits`
    ///     extend `roll_counts` with `changes.roll_changes`
    ///         delete all entries from `roll_counts` for which the roll count is zero
    ///     add each element of `changes.production_stats` to the cycle's `production_stats`
    /// for each `changes.deferred_credits` targeting cycle Ct:
    ///     overwrite `self.deferred_credits` entries of cycle Ct in `cycle_history` with the ones from change
    ///         remove entries for which Amount = 0
    /// if slot S was the last of cycle C:
    ///     set complete=true for cycle C in the history
    ///     compute the seed hash and notifies the `PoSDrawer` for cycle `C+3`
    ///
    pub fn apply_changes_to_batch(
        &mut self,
        changes: PoSChanges,
        slot: Slot,
        feed_selector: bool,
        batch: &mut DBBatch,
    ) -> PosResult<()> {
        let slots_per_cycle: usize = self
            .config
            .periods_per_cycle
            .saturating_mul(self.config.thread_count as u64)
            .try_into()
            .unwrap();

        // compute the current cycle from the given slot
        let cycle = slot.get_cycle(self.config.periods_per_cycle);

        // if cycle C is absent from self.cycle_history:
        // push a new empty CycleInfo at the back of self.cycle_history and set its cycle = C
        // pop_front from cycle_history until front() represents cycle C-4 or later
        // (not C-3 because we might need older endorsement draws on the limit between 2 cycles)
        if let Some(info) = self.cycle_history_cache.back() {
            if cycle == info.0 && !info.1 {
                // extend the last incomplete cycle
            } else if info.0.checked_add(1) == Some(cycle) && info.1 {
                // the previous cycle is complete, push a new incomplete/empty one to extend

                let roll_counts = self.get_all_roll_counts(info.0);
                self.put_new_cycle_info(
                    &CycleInfo::new_with_hash(
                        cycle,
                        false,
                        roll_counts,
                        BitVec::with_capacity(slots_per_cycle),
                        PreHashMap::default(),
                    ),
                    batch,
                );
                while self.cycle_history_cache.len() > self.config.cycle_history_length {
                    if let Some((old_cycle, _)) = self.cycle_history_cache.pop_front() {
                        self.delete_cycle_info(old_cycle, batch);
                    }
                }
            } else {
                return Err(PosError::OverflowError(
                    "invalid cycle sequence in PoS final state".into(),
                ));
            }
        } else {
            return Err(PosError::ContainerInconsistency(
                "PoS history should never be empty here".into(),
            ));
        }

        let complete: bool =
            slot.is_last_of_cycle(self.config.periods_per_cycle, self.config.thread_count);
        self.put_cycle_history_complete(cycle, complete, batch);

        // OPTIM: we could avoid reading the previous seed bits with a cache or with an update function
        let mut rng_seed = self.get_cycle_history_rng_seed(cycle);
        rng_seed.extend(changes.seed_bits);
        self.put_cycle_history_rng_seed(cycle, rng_seed.clone(), batch);

        // extend roll counts
        for (addr, roll_count) in changes.roll_changes {
            self.put_cycle_history_address_entry(cycle, &addr, Some(&roll_count), None, batch);
        }

        // extend production stats
        for (addr, stats) in changes.production_stats {
            if let Some(prev_production_stats) = self.get_production_stats_for_address(cycle, addr)
            {
                let mut new_production_stats = prev_production_stats;
                new_production_stats.extend(&stats);
                self.put_cycle_history_address_entry(
                    cycle,
                    &addr,
                    None,
                    Some(&new_production_stats),
                    batch,
                );
            } else {
                self.put_cycle_history_address_entry(cycle, &addr, None, Some(&stats), batch);
            }
        }

        // if the cycle just completed, check that it has the right number of seed bits
        if complete && rng_seed.len() != slots_per_cycle {
            panic!(
                "cycle completed with incorrect number of seed bits: {} instead of {}",
                rng_seed.len(),
                slots_per_cycle
            );
        }

        // extend deferred_credits with changes.deferred_credits and remove zeros
        for (slot, credits) in changes.deferred_credits.credits.iter() {
            for (address, amount) in credits.iter() {
                self.put_deferred_credits_entry(slot, address, amount, batch);
            }
        }

        // feed the cycle if it is complete
        // notify the PoSDrawer about the newly ready draw data
        // to draw cycle + 2, we use the rng data from cycle - 1 and the seed from cycle
        debug!(
            "After slot {} PoS cycle list is {:?}",
            slot, self.cycle_history_cache
        );
        if complete && feed_selector {
            self.feed_selector(cycle.checked_add(2).ok_or_else(|| {
                PosError::OverflowError("cycle overflow when feeding selector".into())
            })?)
        } else {
            Ok(())
        }
    }

    /// Feeds the selector targeting a given draw cycle
    pub fn feed_selector(&self, draw_cycle: u64) -> PosResult<()> {
        // get roll lookback

        let (lookback_rolls, lookback_state_hash) = match draw_cycle.checked_sub(3) {
            // looking back in history
            Some(c) => {
                let index = self
                    .get_cycle_index(c)
                    .ok_or(PosError::CycleUnavailable(c))?;
                let cycle_info = &self.cycle_history_cache[index];
                if !cycle_info.1 {
                    return Err(PosError::CycleUnfinished(c));
                }
                // take the final_state_hash_snapshot at cycle - 3
                // it will later be combined with rng_seed from cycle - 2 to determine the selection seed
                // do this here to avoid a potential attacker manipulating the selections
                let state_hash = self.get_cycle_history_final_state_hash_snapshot(cycle_info.0);
                (
                    self.get_all_roll_counts(cycle_info.0),
                    Some(state_hash.expect(
                        "critical: a complete cycle must contain a final state hash snapshot",
                    )),
                )
            }
            // looking back to negative cycles
            None => (self.initial_rolls.clone(), None),
        };

        // get seed lookback
        let lookback_seed = match draw_cycle.checked_sub(2) {
            // looking back in history
            Some(c) => {
                let index = self
                    .get_cycle_index(c)
                    .ok_or(PosError::CycleUnavailable(c))?;
                let cycle_info = &self.cycle_history_cache[index];
                if !cycle_info.1 {
                    return Err(PosError::CycleUnfinished(c));
                }
                let u64_ser = U64VarIntSerializer::new();
                let mut seed = Vec::new();
                u64_ser.serialize(&c, &mut seed).unwrap();
                seed.extend(self.get_cycle_history_rng_seed(cycle_info.0).into_vec());
                if let Some(lookback_state_hash) = lookback_state_hash {
                    seed.extend(lookback_state_hash.to_bytes());
                }
                Hash::compute_from(&seed)
            }
            // looking back to negative cycles
            None => self.initial_seeds[draw_cycle as usize],
        };

        // feed selector
        self.selector
            .as_ref()
            .feed_cycle(draw_cycle, lookback_rolls, lookback_seed)
    }

    /// Feeds the selector targeting a given draw cycle
    pub fn feed_cycle_state_hash(&self, cycle: u64, final_state_hash: Hash) {
        if self.get_cycle_index(cycle).is_some() {
            let mut batch = DBBatch::new();
            self.put_cycle_history_final_state_hash_snapshot(
                cycle,
                Some(final_state_hash),
                &mut batch,
            );

            self.db.write().write_batch(batch, Default::default(), None);
        } else {
            panic!("cycle {} should be contained here", cycle);
        }
    }
}

// RocksDB getters
impl PoSFinalState {
    /// Retrieves the amount of rolls a given address has at the latest cycle
    pub fn get_rolls_for(&self, addr: &Address) -> u64 {
        self.cycle_history_cache
            .back()
            .and_then(|info| {
                let cycle = info.0;
                let db = self.db.read();

                let key = roll_count_key!(self.cycle_history_cycle_prefix(cycle), addr);

                if let Some(serialized_value) =
                    db.get_cf(STATE_CF, key).expect(CYCLE_HISTORY_DESER_ERROR)
                {
                    let (_, amount) = self
                        .cycle_info_deserializer
                        .cycle_info_deserializer
                        .rolls_deser
                        .u64_deserializer
                        .deserialize::<DeserializeError>(&serialized_value)
                        .expect(CYCLE_HISTORY_DESER_ERROR);

                    Some(amount)
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }

    /// Retrieves the amount of rolls a given address has at a given cycle
    pub fn get_address_active_rolls(&self, addr: &Address, cycle: u64) -> Option<u64> {
        match cycle.checked_sub(3) {
            Some(lookback_cycle) => {
                let db = self.db.read();

                let key = roll_count_key!(self.cycle_history_cycle_prefix(lookback_cycle), addr);

                if let Some(serialized_value) =
                    db.get_cf(STATE_CF, key).expect(CYCLE_HISTORY_DESER_ERROR)
                {
                    let (_, amount) = self
                        .cycle_info_deserializer
                        .cycle_info_deserializer
                        .rolls_deser
                        .u64_deserializer
                        .deserialize::<DeserializeError>(&serialized_value)
                        .expect(CYCLE_HISTORY_DESER_ERROR);

                    Some(amount)
                } else {
                    None
                }
            }
            None => self.initial_rolls.get(addr).cloned(),
        }
    }

    /// Retrieves every deferred credit in a slot range
    pub fn get_deferred_credits_range<R>(&self, range: R) -> DeferredCredits
    where
        R: RangeBounds<Slot>,
    {
        let db = self.db.read();

        let mut deferred_credits = DeferredCredits::new_without_hash();

        let mut start_key_buffer = Vec::new();
        start_key_buffer.extend_from_slice(DEFERRED_CREDITS_PREFIX.as_bytes());

        match range.start_bound() {
            Included(slot) => {
                self.deferred_credits_serializer
                    .slot_ser
                    .serialize(slot, &mut start_key_buffer)
                    .expect(DEFERRED_CREDITS_SER_ERROR);
            }
            Excluded(slot) => {
                self.deferred_credits_serializer
                    .slot_ser
                    .serialize(
                        &slot
                            .get_next_slot(self.config.thread_count)
                            .expect(DEFERRED_CREDITS_SER_ERROR),
                        &mut start_key_buffer,
                    )
                    .expect(DEFERRED_CREDITS_SER_ERROR);
            }
            _ => {}
        };

        for (serialized_key, serialized_value) in db.iterator_cf(
            STATE_CF,
            MassaIteratorMode::From(&start_key_buffer, MassaDirection::Forward),
        ) {
            if !serialized_key.starts_with(DEFERRED_CREDITS_PREFIX.as_bytes()) {
                break;
            }
            let (rest, slot) = self
                .deferred_credits_deserializer
                .slot_deserializer
                .deserialize::<DeserializeError>(&serialized_key[DEFERRED_CREDITS_PREFIX.len()..])
                .expect(DEFERRED_CREDITS_DESER_ERROR);
            if !range.contains(&slot) {
                break;
            }

            let (_, address) = self
                .deferred_credits_deserializer
                .credit_deserializer
                .address_deserializer
                .deserialize::<DeserializeError>(rest)
                .expect(DEFERRED_CREDITS_DESER_ERROR);

            let (_, amount) = self
                .deferred_credits_deserializer
                .credit_deserializer
                .amount_deserializer
                .deserialize::<DeserializeError>(&serialized_value)
                .expect(DEFERRED_CREDITS_DESER_ERROR);

            deferred_credits.insert(slot, address, amount);
        }

        deferred_credits
    }

    /// Gets the deferred credits for an address
    pub fn get_address_deferred_credits(&self, address: &Address) -> BTreeMap<Slot, Amount> {
        let db = self.db.read();

        let mut deferred_credits = DeferredCredits::new_without_hash();

        let mut start_key_buffer = DEFERRED_CREDITS_PREFIX.as_bytes().to_vec();

        for (serialized_key, serialized_value) in db.iterator_cf(
            STATE_CF,
            MassaIteratorMode::From(&start_key_buffer, MassaDirection::Forward),
        ) {
            if !serialized_key.starts_with(DEFERRED_CREDITS_PREFIX.as_bytes()) {
                break;
            }
            let (rest, slot) = self
                .deferred_credits_deserializer
                .slot_deserializer
                .deserialize::<DeserializeError>(&serialized_key[DEFERRED_CREDITS_PREFIX.len()..])
                .expect(DEFERRED_CREDITS_DESER_ERROR);

            let (_, address) = self
                .deferred_credits_deserializer
                .credit_deserializer
                .address_deserializer
                .deserialize::<DeserializeError>(rest)
                .expect(DEFERRED_CREDITS_DESER_ERROR);

            if address != *address {
                // TODO improve performance
                continue;
            }

            let (_, amount) = self
                .deferred_credits_deserializer
                .credit_deserializer
                .amount_deserializer
                .deserialize::<DeserializeError>(&serialized_value)
                .expect(DEFERRED_CREDITS_DESER_ERROR);

            deferred_credits.insert(slot, address, amount);
        }

        deferred_credits
    }

    /// Gets the index of a cycle in history
    pub fn get_cycle_index(&self, cycle: u64) -> Option<usize> {
        let first_cycle = match self.cycle_history_cache.front() {
            Some(c) => c.0,
            None => return None, // history empty
        };
        if cycle < first_cycle {
            return None; // in the past
        }
        let index: usize = match (cycle - first_cycle).try_into() {
            Ok(v) => v,
            Err(_) => return None, // usize overflow
        };
        if index >= self.cycle_history_cache.len() {
            return None; // in the future
        }
        Some(index)
    }

    /// Get all the roll counts for a given cycle
    pub fn get_all_roll_counts(&self, cycle: u64) -> BTreeMap<Address, u64> {
        let db = self.db.read();

        let mut roll_counts: BTreeMap<Address, u64> = BTreeMap::new();

        let prefix = roll_count_prefix!(self.cycle_history_cycle_prefix(cycle));
        for (serialized_key, serialized_value) in db.prefix_iterator_cf(STATE_CF, &prefix) {
            if !serialized_key.starts_with(prefix.as_bytes()) {
                break;
            }

            let (rest, _cycle) = self
                .cycle_info_deserializer
                .cycle_info_deserializer
                .u64_deser
                .deserialize::<DeserializeError>(&serialized_key[CYCLE_HISTORY_PREFIX.len()..])
                .expect(CYCLE_HISTORY_DESER_ERROR);

            let (_, address) = self
                .cycle_info_deserializer
                .cycle_info_deserializer
                .rolls_deser
                .address_deserializer
                .deserialize::<DeserializeError>(&rest[1..])
                .expect(CYCLE_HISTORY_DESER_ERROR);

            let (_, amount) = self
                .cycle_info_deserializer
                .cycle_info_deserializer
                .rolls_deser
                .u64_deserializer
                .deserialize::<DeserializeError>(&serialized_value)
                .expect(CYCLE_HISTORY_DESER_ERROR);

            roll_counts.insert(address, amount);
        }

        roll_counts
    }

    /// Retrieves the productions statistics for all addresses on a given cycle
    pub fn get_all_production_stats(
        &self,
        cycle: u64,
    ) -> Option<PreHashMap<Address, ProductionStats>> {
        self.get_cycle_index(cycle)
            .map(|idx| self.get_all_production_stats_private(self.cycle_history_cache[idx].0))
    }

    /// Retrieves the productions statistics for all addresses on a given cycle
    pub fn get_all_production_stats_private(
        &self,
        cycle: u64,
    ) -> PreHashMap<Address, ProductionStats> {
        let db = self.db.read();

        let mut production_stats: PreHashMap<Address, ProductionStats> = PreHashMap::default();
        let mut cur_production_stat = ProductionStats::default();
        let mut cur_address = None;

        let prefix = prod_stats_prefix!(self.cycle_history_cycle_prefix(cycle));
        for (serialized_key, serialized_value) in db.prefix_iterator_cf(STATE_CF, &prefix) {
            if !serialized_key.starts_with(prefix.as_bytes()) {
                break;
            }
            let (rest, _cycle) = self
                .cycle_info_deserializer
                .cycle_info_deserializer
                .u64_deser
                .deserialize::<DeserializeError>(&serialized_key[CYCLE_HISTORY_PREFIX.len()..])
                .expect(CYCLE_HISTORY_DESER_ERROR);

            let (rest, address) = self
                .cycle_info_deserializer
                .cycle_info_deserializer
                .production_stats_deser
                .address_deserializer
                .deserialize::<DeserializeError>(&rest[1..])
                .expect(CYCLE_HISTORY_DESER_ERROR);

            if cur_address != Some(address) {
                cur_address = Some(address);
                cur_production_stat = ProductionStats::default();
            }

            let (_, value) = self
                .cycle_info_deserializer
                .cycle_info_deserializer
                .production_stats_deser
                .u64_deserializer
                .deserialize::<DeserializeError>(&serialized_value)
                .expect(CYCLE_HISTORY_DESER_ERROR);

            if rest.len() == 1 && rest[0] == PROD_STATS_FAIL_IDENT {
                cur_production_stat.block_failure_count = value;
            } else if rest.len() == 1 && rest[0] == PROD_STATS_SUCCESS_IDENT {
                cur_production_stat.block_success_count = value;
            } else {
                panic!("{}", CYCLE_HISTORY_DESER_ERROR);
            }

            production_stats.insert(address, cur_production_stat);
        }

        production_stats
    }

    /// Getter for the rng_seed of a given cycle, prioritizing the cache and querying the database as fallback.
    ///
    /// Panics if the cycle is not in the history.
    fn get_cycle_history_rng_seed(&self, cycle: u64) -> BitVec<u8> {
        let db = self.db.read();

        if let Some((cached_cycle, rng_seed)) = &self.rng_seed_cache && *cached_cycle == cycle {
            return rng_seed.clone();
        }

        let serialized_rng_seed = db
            .get_cf(
                STATE_CF,
                rng_seed_key!(self.cycle_history_cycle_prefix(cycle)),
            )
            .expect(CYCLE_HISTORY_DESER_ERROR)
            .expect(CYCLE_HISTORY_DESER_ERROR);

        let (_, rng_seed) = self
            .cycle_info_deserializer
            .cycle_info_deserializer
            .bitvec_deser
            .deserialize::<DeserializeError>(&serialized_rng_seed)
            .expect(CYCLE_HISTORY_DESER_ERROR);

        rng_seed
    }

    /// Getter for the final_state_hash_snapshot of a given cycle.
    ///
    /// Panics if the cycle is not in the history.
    fn get_cycle_history_final_state_hash_snapshot(&self, cycle: u64) -> Option<Hash> {
        let db = self.db.read();

        let serialized_state_hash = db
            .get_cf(
                STATE_CF,
                final_state_hash_snapshot_key!(self.cycle_history_cycle_prefix(cycle)),
            )
            .expect(CYCLE_HISTORY_DESER_ERROR)
            .expect(CYCLE_HISTORY_DESER_ERROR);
        let (_, state_hash) = self
            .cycle_info_deserializer
            .cycle_info_deserializer
            .opt_hash_deser
            .deserialize::<DeserializeError>(&serialized_state_hash)
            .expect(CYCLE_HISTORY_DESER_ERROR);
        state_hash
    }

    /// Used to recompute the cycle cache from the disk.
    ///
    fn get_cycle_history_cycles(&self) -> Vec<(u64, bool)> {
        let db = self.db.read();

        let mut found_cycles: Vec<(u64, bool)> = Vec::new();

        while let Some((serialized_key, _)) = match found_cycles.last() {
            Some((prev_cycle, _)) => db
                .iterator_cf(
                    STATE_CF,
                    MassaIteratorMode::From(
                        &self.cycle_history_cycle_prefix(prev_cycle.saturating_add(1)),
                        MassaDirection::Forward,
                    ),
                )
                .next(),
            None => db
                .iterator_cf(
                    STATE_CF,
                    MassaIteratorMode::From(
                        CYCLE_HISTORY_PREFIX.as_bytes(),
                        MassaDirection::Forward,
                    ),
                )
                .next(),
        } {
            if !serialized_key.starts_with(CYCLE_HISTORY_PREFIX.as_bytes()) {
                break;
            }
            let (_, cycle) = self
                .cycle_info_deserializer
                .cycle_info_deserializer
                .u64_deser
                .deserialize::<DeserializeError>(&serialized_key[CYCLE_HISTORY_PREFIX.len()..])
                .expect(CYCLE_HISTORY_DESER_ERROR);

            found_cycles.push((cycle, self.is_cycle_complete(cycle)));
        }

        found_cycles
    }

    /// Queries a given cycle info in the database
    /// Panics if the cycle is not on disk
    pub fn get_cycle_info(&self, cycle: u64) -> CycleInfo {
        let complete = self.is_cycle_complete(cycle);
        let rng_seed = self.get_cycle_history_rng_seed(cycle);
        let final_state_hash_snapshot = self.get_cycle_history_final_state_hash_snapshot(cycle);

        let roll_counts = self.get_all_roll_counts(cycle);
        let production_stats = self
            .get_all_production_stats(cycle)
            .unwrap_or(PreHashMap::default());

        let mut cycle_info =
            CycleInfo::new_with_hash(cycle, complete, roll_counts, rng_seed, production_stats);
        cycle_info.final_state_hash_snapshot = final_state_hash_snapshot;
        cycle_info
    }

    /// Gets the deferred credits for a given address that will be credited at a given slot
    pub fn get_address_credits_for_slot(&self, addr: &Address, slot: &Slot) -> Option<Amount> {
        let db = self.db.read();

        let mut serialized_key = Vec::new();
        self.deferred_credits_serializer
            .slot_ser
            .serialize(slot, &mut serialized_key)
            .expect(DEFERRED_CREDITS_SER_ERROR);
        self.deferred_credits_serializer
            .credits_ser
            .address_ser
            .serialize(addr, &mut serialized_key)
            .expect(DEFERRED_CREDITS_SER_ERROR);

        match db.get_cf(STATE_CF, deferred_credits_key!(serialized_key)) {
            Ok(Some(serialized_amount)) => {
                let (_, amount) = self
                    .deferred_credits_deserializer
                    .credit_deserializer
                    .amount_deserializer
                    .deserialize::<DeserializeError>(&serialized_amount)
                    .expect(DEFERRED_CREDITS_DESER_ERROR);
                Some(amount)
            }
            _ => None,
        }
    }

    /// Gets the production stats for a given address
    pub fn get_production_stats_for_address(
        &self,
        cycle: u64,
        address: Address,
    ) -> Option<ProductionStats> {
        let db = self.db.read();

        let prefix = self.cycle_history_cycle_prefix(cycle);

        let query = vec![
            (STATE_CF, prod_stats_fail_key!(prefix, address)),
            (STATE_CF, prod_stats_success_key!(prefix, address)),
        ];

        let results = db.multi_get_cf(query);

        match (results.get(0), results.get(1)) {
            (Some(Ok(Some(serialized_fail))), Some(Ok(Some(serialized_success)))) => {
                let (_, fail) = self
                    .cycle_info_deserializer
                    .cycle_info_deserializer
                    .production_stats_deser
                    .u64_deserializer
                    .deserialize::<DeserializeError>(serialized_fail)
                    .expect(CYCLE_HISTORY_DESER_ERROR);
                let (_, success) = self
                    .cycle_info_deserializer
                    .cycle_info_deserializer
                    .production_stats_deser
                    .u64_deserializer
                    .deserialize::<DeserializeError>(serialized_success)
                    .expect(CYCLE_HISTORY_DESER_ERROR);

                Some(ProductionStats {
                    block_success_count: success,
                    block_failure_count: fail,
                })
            }
            _ => None,
        }
    }

    fn is_cycle_complete(&self, cycle: u64) -> bool {
        let db = self.db.read();

        let prefix = self.cycle_history_cycle_prefix(cycle);

        if let Ok(Some(complete_value)) = db.get_cf(STATE_CF, complete_key!(prefix)) {
            complete_value.len() == 1 && complete_value[0] == 1
        } else {
            false
        }
    }
}

// RocksDB setters
impl PoSFinalState {
    /// Helper function to put a new CycleInfo to RocksDB, and update the cycle_history cache
    fn put_new_cycle_info(&mut self, cycle_info: &CycleInfo, batch: &mut DBBatch) {
        self.put_cycle_history_complete(cycle_info.cycle, cycle_info.complete, batch);
        self.put_cycle_history_rng_seed(cycle_info.cycle, cycle_info.rng_seed.clone(), batch);
        self.put_cycle_history_final_state_hash_snapshot(
            cycle_info.cycle,
            cycle_info.final_state_hash_snapshot,
            batch,
        );
        for (address, roll) in cycle_info.roll_counts.iter() {
            self.put_cycle_history_address_entry(
                cycle_info.cycle,
                address,
                Some(roll),
                None,
                batch,
            );
        }
        for (address, prod_stats) in cycle_info.production_stats.iter() {
            self.put_cycle_history_address_entry(
                cycle_info.cycle,
                address,
                None,
                Some(prod_stats),
                batch,
            );
        }
        self.cycle_history_cache
            .push_back((cycle_info.cycle, cycle_info.complete));
    }

    /// Helper function to put a the complete flag for a given cycle
    fn put_cycle_history_complete(&mut self, cycle: u64, value: bool, batch: &mut DBBatch) {
        let db = self.db.read();

        let prefix = self.cycle_history_cycle_prefix(cycle);

        let serialized_value = if value { &[1] } else { &[0] };

        db.put_or_update_entry_value(batch, complete_key!(prefix), serialized_value);

        if let Some(index) = self.get_cycle_index(cycle) {
            self.cycle_history_cache[index].1 = value;
        }
    }

    /// Helper function to put a the final_state_hash_snapshot for a given cycle
    fn put_cycle_history_final_state_hash_snapshot(
        &self,
        cycle: u64,
        value: Option<Hash>,
        batch: &mut DBBatch,
    ) {
        let db = self.db.read();

        let prefix = self.cycle_history_cycle_prefix(cycle);

        let mut serialized_value = Vec::new();
        self.cycle_info_serializer
            .cycle_info_serializer
            .opt_hash_ser
            .serialize(&value, &mut serialized_value)
            .expect(CYCLE_HISTORY_SER_ERROR);

        db.put_or_update_entry_value(
            batch,
            final_state_hash_snapshot_key!(prefix),
            &serialized_value,
        );
    }

    /// Helper function to put a the rng_seed for a given cycle
    fn put_cycle_history_rng_seed(&mut self, cycle: u64, value: BitVec<u8>, batch: &mut DBBatch) {
        let db = self.db.read();

        let prefix = self.cycle_history_cycle_prefix(cycle);

        let mut serialized_value = Vec::new();
        self.cycle_info_serializer
            .cycle_info_serializer
            .bitvec_ser
            .serialize(&value, &mut serialized_value)
            .expect(CYCLE_HISTORY_SER_ERROR);

        self.rng_seed_cache = Some((cycle, value.clone()));

        db.put_or_update_entry_value(batch, rng_seed_key!(prefix), &serialized_value);
    }

    /// Internal function to put an entry for a given address in the cycle history
    fn put_cycle_history_address_entry(
        &self,
        cycle: u64,
        address: &Address,
        roll_count: Option<&u64>,
        production_stats: Option<&ProductionStats>,
        batch: &mut DBBatch,
    ) {
        let db = self.db.read();

        let prefix = self.cycle_history_cycle_prefix(cycle);

        // Roll count
        if let Some(0) = roll_count {
            db.delete_key(batch, roll_count_key!(prefix, address));
        } else if let Some(roll_count) = roll_count {
            let mut serialized_roll_count = Vec::new();
            self.cycle_info_serializer
                .cycle_info_serializer
                .u64_ser
                .serialize(roll_count, &mut serialized_roll_count)
                .expect(CYCLE_HISTORY_SER_ERROR);
            db.put_or_update_entry_value(
                batch,
                roll_count_key!(prefix, address),
                &serialized_roll_count,
            );
        }

        // Production stats
        if let Some(production_stats) = production_stats {
            let mut serialized_prod_stats_fail = Vec::new();
            self.cycle_info_serializer
                .cycle_info_serializer
                .u64_ser
                .serialize(
                    &production_stats.block_failure_count,
                    &mut serialized_prod_stats_fail,
                )
                .expect(CYCLE_HISTORY_SER_ERROR);
            db.put_or_update_entry_value(
                batch,
                prod_stats_fail_key!(prefix, address),
                &serialized_prod_stats_fail,
            );

            // Production stats success
            let mut serialized_prod_stats_success = Vec::new();
            self.cycle_info_serializer
                .cycle_info_serializer
                .u64_ser
                .serialize(
                    &production_stats.block_success_count,
                    &mut serialized_prod_stats_success,
                )
                .expect(CYCLE_HISTORY_SER_ERROR);
            db.put_or_update_entry_value(
                batch,
                prod_stats_success_key!(prefix, address),
                &serialized_prod_stats_success,
            );
        }
    }

    /// Internal function to put an entry
    pub fn put_deferred_credits_entry(
        &self,
        slot: &Slot,
        address: &Address,
        amount: &Amount,
        batch: &mut DBBatch,
    ) {
        let db = self.db.read();

        let mut serialized_key = Vec::new();
        self.deferred_credits_serializer
            .slot_ser
            .serialize(slot, &mut serialized_key)
            .expect(DEFERRED_CREDITS_SER_ERROR);
        self.deferred_credits_serializer
            .credits_ser
            .address_ser
            .serialize(address, &mut serialized_key)
            .expect(DEFERRED_CREDITS_SER_ERROR);

        if amount.is_zero() {
            db.delete_key(batch, deferred_credits_key!(serialized_key));
        } else {
            let mut serialized_amount = Vec::new();
            self.deferred_credits_serializer
                .credits_ser
                .amount_ser
                .serialize(amount, &mut serialized_amount)
                .expect(DEFERRED_CREDITS_SER_ERROR);

            db.put_or_update_entry_value(
                batch,
                deferred_credits_key!(serialized_key),
                &serialized_amount,
            );
        }
    }
}

/// Helpers for key and value management
impl PoSFinalState {
    /// Helper function to construct the key prefix associated with a given cycle
    fn cycle_history_cycle_prefix(&self, cycle: u64) -> Vec<u8> {
        let mut serialized_key = Vec::new();
        serialized_key.extend_from_slice(CYCLE_HISTORY_PREFIX.as_bytes());
        self.cycle_info_serializer
            .cycle_info_serializer
            .u64_ser
            .serialize(&cycle, &mut serialized_key)
            .expect(CYCLE_HISTORY_SER_ERROR);
        serialized_key
    }

    /// Deserializes the key and value, useful after bootstrap
    pub fn is_cycle_history_key_value_valid(
        &self,
        serialized_key: &[u8],
        serialized_value: &[u8],
    ) -> bool {
        if !serialized_key.starts_with(CYCLE_HISTORY_PREFIX.as_bytes()) {
            return false;
        }

        let Ok((rest, _cycle)) = self
            .cycle_info_deserializer
            .cycle_info_deserializer
            .u64_deser
            .deserialize::<DeserializeError>(&serialized_key[CYCLE_HISTORY_PREFIX.len()..]) else {
            return false;
        };

        if rest.is_empty() {
            return false;
        }

        match rest[0] {
            COMPLETE_IDENT => {
                if rest.len() != 1 {
                    return false;
                }
                if serialized_value.len() != 1 {
                    return false;
                }
                if serialized_value[0] > 1 {
                    return false;
                }
            }
            RNG_SEED_IDENT => {
                if rest.len() != 1 {
                    return false;
                }
                let Ok((rest, _rng_seed)) = self
                    .cycle_info_deserializer
                    .cycle_info_deserializer
                    .bitvec_deser
                    .deserialize::<DeserializeError>(serialized_value) else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            FINAL_STATE_HASH_SNAPSHOT_IDENT => {
                if rest.len() != 1 {
                    return false;
                }
                let Ok((rest, _final_state_hash)) = self
                    .cycle_info_deserializer
                    .cycle_info_deserializer
                    .opt_hash_deser
                    .deserialize::<DeserializeError>(serialized_value) else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            ROLL_COUNT_IDENT => {
                let Ok((rest, _addr)): std::result::Result<(&[u8], Address), nom::Err<massa_serialization::DeserializeError<'_>>> = self
                    .cycle_info_deserializer
                    .cycle_info_deserializer
                    .rolls_deser
                    .address_deserializer
                    .deserialize::<DeserializeError>(&rest[1..]) else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
                let Ok((rest, _addr)) = self
                    .cycle_info_deserializer
                    .cycle_info_deserializer
                    .rolls_deser
                    .u64_deserializer
                    .deserialize::<DeserializeError>(serialized_value) else {
                    return false;
                };
                if !rest.is_empty() {
                    return false;
                }
            }
            PROD_STATS_IDENT => {
                let Ok((rest, _addr)): std::result::Result<(&[u8], Address), nom::Err<massa_serialization::DeserializeError<'_>>> = self
                    .cycle_info_deserializer
                    .cycle_info_deserializer
                    .rolls_deser
                    .address_deserializer
                    .deserialize::<DeserializeError>(&rest[1..]) else {
                        return false;
                    };
                if rest.len() != 1 {
                    return false;
                }

                match rest[0] {
                    PROD_STATS_FAIL_IDENT => {
                        let Ok((rest, _fail)) = self
                            .cycle_info_deserializer
                            .cycle_info_deserializer
                            .production_stats_deser
                            .u64_deserializer
                            .deserialize::<DeserializeError>(serialized_value) else {
                                return false;
                            };
                        if !rest.is_empty() {
                            return false;
                        }
                    }
                    PROD_STATS_SUCCESS_IDENT => {
                        let Ok((rest, _success)) = self
                        .cycle_info_deserializer
                        .cycle_info_deserializer
                        .production_stats_deser
                        .u64_deserializer
                        .deserialize::<DeserializeError>(serialized_value) else {
                            return false;
                        };
                        if !rest.is_empty() {
                            return false;
                        }
                    }
                    _ => {
                        return false;
                    }
                }
            }
            _ => {
                return false;
            }
        }

        true
    }

    /// Deserializes the key and value, useful after bootstrap
    pub fn is_deferred_credits_key_value_valid(
        &self,
        serialized_key: &[u8],
        serialized_value: &[u8],
    ) -> bool {
        if !serialized_key.starts_with(DEFERRED_CREDITS_PREFIX.as_bytes()) {
            return false;
        }

        let Ok((rest, _slot)) = self.deferred_credits_deserializer.slot_deserializer.deserialize::<DeserializeError>(&serialized_key[DEFERRED_CREDITS_PREFIX.len()..]) else {
            return false;
        };
        let Ok((rest, _addr)): std::result::Result<(&[u8], Address), nom::Err<massa_serialization::DeserializeError<'_>>> = self.deferred_credits_deserializer.credit_deserializer.address_deserializer.deserialize::<DeserializeError>(rest) else {
            return false;
        };
        if !rest.is_empty() {
            return false;
        }

        let Ok((rest, _mount)) = self.deferred_credits_deserializer.credit_deserializer.amount_deserializer.deserialize::<DeserializeError>(serialized_value) else {
            return false;
        };
        if !rest.is_empty() {
            return false;
        }

        true
    }
}

/// Helpers for testing
#[cfg(feature = "testing")]
impl PoSFinalState {
    /// Queries all the deferred credits in the database
    pub fn get_deferred_credits(&self) -> DeferredCredits {
        let db = self.db.read();

        let mut deferred_credits = DeferredCredits::new_with_hash();

        for (serialized_key, serialized_value) in
            db.prefix_iterator_cf(STATE_CF, DEFERRED_CREDITS_PREFIX.as_bytes())
        {
            if !serialized_key.starts_with(DEFERRED_CREDITS_PREFIX.as_bytes()) {
                break;
            }
            let (rest, slot) = self
                .deferred_credits_deserializer
                .slot_deserializer
                .deserialize::<DeserializeError>(&serialized_key[DEFERRED_CREDITS_PREFIX.len()..])
                .expect(DEFERRED_CREDITS_DESER_ERROR);
            let (_, address) = self
                .deferred_credits_deserializer
                .credit_deserializer
                .address_deserializer
                .deserialize::<DeserializeError>(&rest)
                .expect(DEFERRED_CREDITS_DESER_ERROR);

            let (_, amount) = self
                .deferred_credits_deserializer
                .credit_deserializer
                .amount_deserializer
                .deserialize::<DeserializeError>(&serialized_value)
                .expect(DEFERRED_CREDITS_DESER_ERROR);

            deferred_credits.insert(slot, address, amount);
        }
        deferred_credits
    }
}

#[test]
fn test_pos_final_state_hash_computation() {
    use crate::test_exports::MockSelectorController;
    use crate::DeferredCredits;
    use crate::PoSFinalState;
    use bitvec::prelude::*;
    use massa_db_exports::{MassaDBConfig, MassaDBController};
    use massa_db_worker::MassaDB;
    use massa_models::config::constants::{
        MAX_DEFERRED_CREDITS_LENGTH, MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH,
        POS_SAVED_CYCLES,
    };
    use massa_signature::KeyPair;
    use parking_lot::RwLock;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::TempDir;

    let pos_config = PoSConfig {
        periods_per_cycle: 2,
        thread_count: 2,
        cycle_history_length: POS_SAVED_CYCLES,
        max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
        max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
        max_credit_length: MAX_DEFERRED_CREDITS_LENGTH,
    };

    // initialize the executed ops config
    let tempdir = TempDir::new().expect("cannot create temp directory");
    let db_config = MassaDBConfig {
        path: tempdir.path().to_path_buf(),
        max_history_length: 10,
        max_new_elements: 100,
        thread_count: 2,
    };
    let db = Arc::new(RwLock::new(
        Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
    ));
    let (selector_controller, _) = MockSelectorController::new_with_receiver();
    let init_seed = Hash::compute_from(b"");
    let initial_seeds = vec![Hash::compute_from(init_seed.to_bytes()), init_seed];

    let deferred_credits_deserializer = DeferredCreditsDeserializer::new(
        pos_config.thread_count,
        pos_config.max_credit_length,
        true,
    );
    let cycle_info_deserializer = CycleHistoryDeserializer::new(
        pos_config.cycle_history_length as u64,
        pos_config.max_rolls_length,
        pos_config.max_production_stats_length,
    );

    let mut pos_state = PoSFinalState {
        config: pos_config,
        db: db.clone(),
        cycle_history_cache: Default::default(),
        rng_seed_cache: None,
        selector: selector_controller,
        initial_rolls: Default::default(),
        initial_seeds,
        deferred_credits_serializer: DeferredCreditsSerializer::new(),
        deferred_credits_deserializer,
        cycle_info_serializer: CycleHistorySerializer::new(),
        cycle_info_deserializer,
    };

    pos_state.recompute_pos_state_caches();

    let mut batch = DBBatch::new();
    pos_state.create_initial_cycle(&mut batch);
    db.write()
        .write_batch(batch, Default::default(), Some(Slot::new(0, 0)));

    let addr = Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());

    // add changes
    let mut roll_changes = PreHashMap::default();
    roll_changes.insert(addr, 10);
    let mut production_stats = PreHashMap::default();
    production_stats.insert(
        addr,
        ProductionStats {
            block_success_count: 4,
            block_failure_count: 0,
        },
    );
    let changes = PoSChanges {
        seed_bits: bitvec![u8, Lsb0; 0, 1],
        roll_changes: roll_changes.clone(),
        production_stats: production_stats.clone(),
        deferred_credits: DeferredCredits::new_with_hash(),
    };

    let mut batch = DBBatch::new();
    pos_state
        .apply_changes_to_batch(changes, Slot::new(0, 0), false, &mut batch)
        .unwrap();
    db.write()
        .write_batch(batch, Default::default(), Some(Slot::new(0, 0)));

    // update changes once
    roll_changes.clear();
    roll_changes.insert(addr, 20);
    production_stats.clear();
    production_stats.insert(
        addr,
        ProductionStats {
            block_success_count: 4,
            block_failure_count: 6,
        },
    );
    let changes = PoSChanges {
        seed_bits: bitvec![u8, Lsb0; 1, 0],
        roll_changes: roll_changes.clone(),
        production_stats: production_stats.clone(),
        deferred_credits: DeferredCredits::new_with_hash(),
    };

    let mut batch = DBBatch::new();
    pos_state
        .apply_changes_to_batch(changes, Slot::new(0, 1), false, &mut batch)
        .unwrap();
    db.write()
        .write_batch(batch, Default::default(), Some(Slot::new(0, 1)));

    // update changes twice
    roll_changes.clear();
    roll_changes.insert(addr, 0);
    production_stats.clear();
    production_stats.insert(
        addr,
        ProductionStats {
            block_success_count: 4,
            block_failure_count: 12,
        },
    );

    let changes = PoSChanges {
        seed_bits: bitvec![u8, Lsb0; 0, 1],
        roll_changes,
        production_stats,
        deferred_credits: DeferredCredits::new_with_hash(),
    };

    let mut batch = DBBatch::new();
    pos_state
        .apply_changes_to_batch(changes, Slot::new(1, 0), false, &mut batch)
        .unwrap();
    db.write()
        .write_batch(batch, Default::default(), Some(Slot::new(1, 0)));

    let cycles = pos_state.get_cycle_history_cycles();
    assert_eq!(cycles.len(), 1, "wrong number of cycles");
    assert_eq!(cycles[0].0, 0, "cycle should be the 1st one");
    assert_eq!(cycles[0].1, false, "cycle should not be complete yet");

    let cycle_info_a = pos_state.get_cycle_info(0);

    let mut prod_stats = HashMap::default();
    prod_stats.insert(
        addr,
        ProductionStats {
            block_success_count: 12,
            block_failure_count: 18,
        },
    );

    let cycle_info_b = CycleInfo::new_with_hash(
        0,
        false,
        BTreeMap::default(),
        bitvec![u8, Lsb0; 0, 0, 0, 1, 1, 0, 0, 1],
        prod_stats,
    );

    assert_eq!(
        cycle_info_a.roll_counts_hash, cycle_info_b.roll_counts_hash,
        "roll_counts_hash mismatch"
    );
    assert_eq!(
        cycle_info_a.production_stats_hash, cycle_info_b.production_stats_hash,
        "production_stats_hash mismatch"
    );
    assert_eq!(
        cycle_info_a.cycle_global_hash, cycle_info_b.cycle_global_hash,
        "global_hash mismatch"
    );
}
