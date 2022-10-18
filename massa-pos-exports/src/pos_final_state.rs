use crate::DeferredCredits;
use crate::{CycleInfo, PoSChanges, PosError, PosResult, ProductionStats, SelectorController};
use bitvec::vec::BitVec;
use massa_hash::Hash;
use massa_models::error::ModelsError;
use massa_models::streaming_step::StreamingStep;
use massa_models::{
    address::{Address, AddressDeserializer},
    amount::{Amount, AmountDeserializer},
    prehash::PreHashMap,
    slot::{Slot, SlotDeserializer},
};
use massa_serialization::U64VarIntDeserializer;
use std::collections::VecDeque;
use std::{
    collections::BTreeMap,
    ops::Bound::{Excluded, Included, Unbounded},
    path::PathBuf,
};
use tracing::debug;

/// Final state of PoS
pub struct PoSFinalState {
    /// contiguous cycle history, back = newest
    pub cycle_history: VecDeque<CycleInfo>,
    /// coins to be credited at the end of the slot
    pub deferred_credits: DeferredCredits,
    /// selector controller
    pub selector: Box<dyn SelectorController>,
    /// initial rolls, used for negative cycle look back
    pub initial_rolls: BTreeMap<Address, u64>,
    /// initial seeds, used for negative cycle look back (cycles -2, -1 in that order)
    pub initial_seeds: Vec<Hash>,
    /// amount deserializer
    pub amount_deserializer: AmountDeserializer,
    /// slot deserializer
    pub slot_deserializer: SlotDeserializer,
    /// deserializer
    pub deferred_credit_length_deserializer: U64VarIntDeserializer,
    /// address deserializer
    pub address_deserializer: AddressDeserializer,
    /// periods per cycle
    pub periods_per_cycle: u64,
    /// thread count
    pub thread_count: u8,
}

impl PoSFinalState {
    /// create a new `PoSFinalState`
    pub fn new(
        // FOLLOW-UP TODO: use a config
        initial_seed_string: &String,
        initial_rolls_path: &PathBuf,
        periods_per_cycle: u64,
        thread_count: u8,
        selector: Box<dyn SelectorController>,
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

        // Deserializers
        let amount_deserializer =
            AmountDeserializer::new(Included(Amount::MIN), Included(Amount::MAX));
        let slot_deserializer = SlotDeserializer::new(
            (Included(u64::MIN), Included(u64::MAX)),
            (Included(0), Excluded(thread_count)),
        );
        let deferred_credit_length_deserializer =
            U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)); // TODO define a max here
        let address_deserializer = AddressDeserializer::new();

        Ok(Self {
            cycle_history: Default::default(),
            deferred_credits: DeferredCredits::default(),
            selector,
            initial_rolls,
            initial_seeds,
            amount_deserializer,
            slot_deserializer,
            deferred_credit_length_deserializer,
            address_deserializer,
            periods_per_cycle,
            thread_count,
        })
    }

    /// Create the initial cycle based off the initial rolls.
    ///
    /// This should be called only if bootstrap did not happen.
    pub fn create_initial_cycle(&mut self) {
        let mut rng_seed = BitVec::with_capacity(
            self.periods_per_cycle
                .saturating_mul(self.thread_count as u64)
                .try_into()
                .unwrap(),
        );
        for _ in 0..self.thread_count {
            // assume genesis blocks have a "False" seed bit to avoid passing them around
            rng_seed.push(false);
        }
        self.cycle_history.push_back(CycleInfo {
            cycle: 0,
            rng_seed,
            production_stats: Default::default(),
            roll_counts: self.initial_rolls.clone(),
            complete: false,
        });
    }

    /// Sends the current draw inputs (initial or bootstrapped) to the selector.
    /// Waits for the initial draws to be performed.
    pub fn compute_initial_draws(&mut self) -> PosResult<()> {
        // if cycle_history starts at a cycle that is strictly higher than 0, do not feed cycles 0, 1 to selector
        let history_starts_late = self
            .cycle_history
            .front()
            .map(|c_info| c_info.cycle > 0)
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
        for (idx, hist_item) in self.cycle_history.iter().enumerate() {
            if !hist_item.complete {
                break;
            }
            if history_starts_late && idx == 0 {
                // If the history starts late, the first RNG seed cannot be used to draw
                // because the roll distribution which should be provided by the previous element is absent.
                continue;
            }
            let draw_cycle = hist_item.cycle.checked_add(2).ok_or_else(|| {
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

    /// Technical specification of `apply_changes`:
    ///
    /// set `self.last_final_slot` = C
    /// if cycle C is absent from `self.cycle_history`:
    ///     `push` a new empty `CycleInfo` at the back of `self.cycle_history` and set its cycle = C
    ///     `pop_front` from `cycle_history` until front() represents cycle C-4 or later (not C-3 because we might need older endorsement draws on the limit between 2 cycles)
    /// for the cycle C entry of `cycle_history`:
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
    pub fn apply_changes(
        &mut self,
        changes: PoSChanges,
        slot: Slot,
        feed_selector: bool,
    ) -> PosResult<()> {
        let slots_per_cycle: usize = self
            .periods_per_cycle
            .saturating_mul(self.thread_count as u64)
            .try_into()
            .unwrap();

        // compute the current cycle from the given slot
        let cycle = slot.get_cycle(self.periods_per_cycle);

        // if cycle C is absent from self.cycle_history:
        // push a new empty CycleInfo at the back of self.cycle_history and set its cycle = C
        // pop_front from cycle_history until front() represents cycle C-4 or later
        // (not C-3 because we might need older endorsement draws on the limit between 2 cycles)
        if let Some(info) = self.cycle_history.back() {
            if cycle == info.cycle && !info.complete {
                // extend the last incomplete cycle
            } else if info.cycle.checked_add(1) == Some(cycle) && info.complete {
                // the previous cycle is complete, push a new incomplete/empty one to extend
                self.cycle_history.push_back(CycleInfo {
                    cycle,
                    roll_counts: info.roll_counts.clone(),
                    rng_seed: BitVec::with_capacity(slots_per_cycle),
                    production_stats: Default::default(),
                    complete: false,
                });
                // add 1 for the current cycle and 1 for bootstrap safety
                while self.cycle_history.len() > 6 {
                    self.cycle_history.pop_front();
                }
            } else {
                return Err(PosError::OverflowError(
                    "invalid cycle sequence in PoS final state".into(),
                ));
            }
        } else {
            panic!("PoS History shouldn't be empty here.");
        }

        // update cycle data
        let cycle_completed: bool;
        {
            let current = self
                .cycle_history
                .back_mut()
                .expect("cycle history should be non-empty"); // because if was filled above

            // extend seed_bits with changes.seed_bits
            current.rng_seed.extend(changes.seed_bits);

            // extend roll counts
            current.roll_counts.extend(changes.roll_changes);
            current.roll_counts.retain(|_, &mut count| count != 0);

            // extend production stats
            for (addr, stats) in changes.production_stats {
                current
                    .production_stats
                    .entry(addr)
                    .and_modify(|cur| cur.extend(&stats))
                    .or_insert(stats);
            }

            // check for completion
            current.complete = slot.is_last_of_cycle(self.periods_per_cycle, self.thread_count);
            // if the cycle just completed, check that it has the right number of seed bits
            if current.complete && current.rng_seed.len() != slots_per_cycle {
                panic!("cycle completed with incorrect number of seed bits");
            }
            cycle_completed = current.complete;
        }

        // extent deferred_credits with changes.deferred_credits
        // remove zero-valued credits
        self.deferred_credits
            .nested_extend(changes.deferred_credits);
        self.deferred_credits.remove_zeros();

        // feed the cycle if it is complete
        // notify the PoSDrawer about the newly ready draw data
        // to draw cycle + 2, we use the rng data from cycle - 1 and the seed from cycle
        debug!(
            "After slot {} PoS cycle list is {:?}",
            slot,
            self.cycle_history
                .iter()
                .map(|c| (c.cycle, c.complete))
                .collect::<Vec<(u64, bool)>>()
        );
        if cycle_completed && feed_selector {
            self.feed_selector(cycle.checked_add(2).ok_or_else(|| {
                PosError::OverflowError("cycle overflow when feeding selector".into())
            })?)
        } else {
            Ok(())
        }
    }

    /// Feeds the selector targeting a given draw cycle
    fn feed_selector(&self, draw_cycle: u64) -> PosResult<()> {
        // get roll lookback
        let lookback_rolls = match draw_cycle.checked_sub(3) {
            // looking back in history
            Some(c) => {
                let index = self
                    .get_cycle_index(c)
                    .ok_or(PosError::CycleUnavailable(c))?;
                let cycle_info = &self.cycle_history[index];
                if !cycle_info.complete {
                    return Err(PosError::CycleUnfinished(c));
                }
                cycle_info.roll_counts.clone()
            }
            // looking back to negative cycles
            None => self.initial_rolls.clone(),
        };

        // get seed lookback
        let lookback_seed = match draw_cycle.checked_sub(2) {
            // looking back in history
            Some(c) => {
                let index = self
                    .get_cycle_index(c)
                    .ok_or(PosError::CycleUnavailable(c))?;
                let cycle_info = &self.cycle_history[index];
                if !cycle_info.complete {
                    return Err(PosError::CycleUnfinished(c));
                }
                Hash::compute_from(&cycle_info.rng_seed.clone().into_vec())
            }
            // looking back to negative cycles
            None => self.initial_seeds[draw_cycle as usize],
        };

        // feed selector
        self.selector
            .as_ref()
            .feed_cycle(draw_cycle, lookback_rolls, lookback_seed)
    }

    /// Retrieves the amount of rolls a given address has at the latest cycle
    pub fn get_rolls_for(&self, addr: &Address) -> u64 {
        self.cycle_history
            .back()
            .and_then(|info| info.roll_counts.get(addr).cloned())
            .unwrap_or_default()
    }

    /// Retrieves the amount of rolls a given address has at a given cycle
    pub fn get_address_active_rolls(&self, addr: &Address, cycle: u64) -> Option<u64> {
        // get lookback cycle index
        let lookback_cycle = cycle.checked_sub(3);
        if let Some(lookback_cycle) = lookback_cycle {
            let lookback_index = match self.get_cycle_index(lookback_cycle) {
                Some(idx) => idx,
                None => return None,
            };
            // get rolls
            self.cycle_history[lookback_index]
                .roll_counts
                .get(addr)
                .cloned()
        } else {
            self.initial_rolls.get(addr).cloned()
        }
    }

    /// Retrieves every deferred credit of the given slot
    pub fn get_deferred_credits_at(&self, slot: &Slot) -> PreHashMap<Address, Amount> {
        self.deferred_credits
            .0
            .get(slot)
            .cloned()
            .unwrap_or_default()
    }

    /// Retrieves the productions statistics for all addresses on a given cycle
    pub fn get_all_production_stats(
        &self,
        cycle: u64,
    ) -> Option<&PreHashMap<Address, ProductionStats>> {
        self.get_cycle_index(cycle)
            .map(|idx| &self.cycle_history[idx].production_stats)
    }

    /// Gets the index of a cycle in history
    pub fn get_cycle_index(&self, cycle: u64) -> Option<usize> {
        let first_cycle = match self.cycle_history.front() {
            Some(c) => c.cycle,
            None => return None, // history empty
        };
        if cycle < first_cycle {
            return None; // in the past
        }
        let index: usize = match (cycle - first_cycle).try_into() {
            Ok(v) => v,
            Err(_) => return None, // usize overflow
        };
        if index >= self.cycle_history.len() {
            return None; // in the future
        }
        Some(index)
    }

    /// Gets a cycle of the Proof of Stake `cycle_history`. Used only in the bootstrap process.
    ///
    /// # Arguments:
    /// `cursor`: indicates the bootstrap state after the previous payload
    ///
    /// # Returns
    /// The PoS cycle and the updated cursor
    pub fn get_cycle_history_part(
        &self,
        cursor: StreamingStep<u64>,
    ) -> Result<(Option<CycleInfo>, StreamingStep<u64>), ModelsError> {
        let cycle_index = match cursor {
            // FOLLOW-UP TODO: use the config value for `6`
            StreamingStep::Started => usize::from(self.cycle_history.len() >= 6),
            StreamingStep::Ongoing(last_cycle) => {
                if let Some(index) = self.get_cycle_index(last_cycle) {
                    if index == self.cycle_history.len() - 1 {
                        return Ok((None, StreamingStep::Finished));
                    }
                    index.saturating_add(1)
                } else {
                    return Err(ModelsError::OutdatedBootstrapCursor);
                }
            }
            StreamingStep::Finished => return Ok((None, cursor)),
        };
        let cycle_info = self
            .cycle_history
            .get(cycle_index)
            .expect("a cycle should be available here");
        Ok((
            Some(cycle_info.clone()),
            StreamingStep::Ongoing(cycle_info.cycle),
        ))
    }

    /// Gets a part of the Proof of Stake `deferred_credits`. Used only in the bootstrap process.
    ///
    /// # Arguments:
    /// `cursor`: indicates the bootstrap state after the previous payload
    ///
    /// # Returns
    /// The PoS `deferred_credits` part and the updated cursor
    pub fn get_deferred_credits_part(
        &self,
        cursor: StreamingStep<Slot>,
    ) -> (DeferredCredits, StreamingStep<Slot>) {
        let mut credits_part = DeferredCredits::default();
        let left_bound = match cursor {
            StreamingStep::Started => Unbounded,
            StreamingStep::Ongoing(last_slot) => Excluded(last_slot),
            StreamingStep::Finished => return (credits_part, cursor),
        };
        let mut credit_part_last_slot: Option<Slot> = None;
        for (slot, credits) in self.deferred_credits.0.range((left_bound, Unbounded)) {
            // FOLLOW-UP TODO: stream in multiple parts
            credits_part.0.insert(*slot, credits.clone());
            credit_part_last_slot = Some(*slot);
        }
        if let Some(last_slot) = credit_part_last_slot {
            (credits_part, StreamingStep::Ongoing(last_slot))
        } else {
            (credits_part, StreamingStep::Finished)
        }
    }

    /// Sets a part of the Proof of Stake `cycle_history`. Used only in the bootstrap process.
    ///
    /// # Arguments
    /// `part`: a `CycleInfo` received from `get_pos_state_part` and used to update PoS final state
    pub fn set_cycle_history_part(&mut self, part: Option<CycleInfo>) -> StreamingStep<u64> {
        if let Some(cycle_info) = part {
            let opt_next_cycle = self
                .cycle_history
                .back()
                .map(|info| info.cycle.saturating_add(1));
            let current_cycle = cycle_info.cycle;
            if let Some(next_cycle) = opt_next_cycle && current_cycle != next_cycle {
            panic!("PoS received cycle ({}) should be equal to the next expected cycle ({})", current_cycle, next_cycle);
        }
            self.cycle_history.push_back(cycle_info);
            StreamingStep::Ongoing(current_cycle)
        } else {
            StreamingStep::Finished
        }
    }

    /// Sets a part of the Proof of Stake `deferred_credits`. Used only in the bootstrap process.
    ///
    /// # Arguments
    /// `part`: `DeferredCredits` from `get_pos_state_part` and used to update PoS final state
    pub fn set_deferred_credits_part(&mut self, part: DeferredCredits) -> StreamingStep<Slot> {
        self.deferred_credits.nested_extend(part);
        if let Some(slot) = self
            .deferred_credits
            .0
            .last_key_value()
            .map(|(&slot, _)| slot)
        {
            StreamingStep::Ongoing(slot)
        } else {
            StreamingStep::Finished
        }
    }
}
